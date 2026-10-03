use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_ode_engine::{Request, Response};

static NEXT_TEST_DATABASE: AtomicU64 = AtomicU64::new(0);
const FRAME_DATA: u8 = 1;

struct TestSidecar {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl TestSidecar {
    fn start(database_root: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_worlddb_ode_engine"))
            .arg(database_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("sidecar starts");
        let input = child.stdin.take().expect("sidecar stdin");
        let output = BufReader::new(child.stdout.take().expect("sidecar stdout"));
        let mut sidecar = Self {
            child,
            input,
            output,
        };
        assert!(matches!(
            sidecar.read_response(),
            Response::Ready {
                protocol_version: 1,
                ..
            }
        ));
        sidecar
    }

    fn send_request(&mut self, request: &Request) {
        serde_json::to_writer(&mut self.input, request).expect("request encodes");
        self.input.write_all(b"\n").expect("request line writes");
        self.input.flush().expect("request flushes");
    }

    fn read_response(&mut self) -> Response {
        let mut line = String::new();
        self.output
            .read_line(&mut line)
            .expect("response line reads");
        serde_json::from_str(&line).expect("response decodes")
    }

    fn push_chunk(&mut self, transfer_id: &str, sequence: u64, chunk: &[u8]) -> Response {
        self.send_request(&Request::StreamChunk {
            transfer_id: transfer_id.to_owned(),
            sequence,
            chunk_bytes: chunk.len() as u32,
        });
        self.input
            .write_all(&[FRAME_DATA])
            .expect("frame type writes");
        self.input
            .write_all(&(chunk.len() as u32).to_le_bytes())
            .expect("frame length writes");
        self.input.write_all(chunk).expect("frame payload writes");
        self.input.flush().expect("frame flushes");
        self.read_response()
    }
}

impl Drop for TestSidecar {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = serde_json::to_writer(&mut self.input, &Request::Shutdown);
        let _ = self.input.write_all(b"\n");
        let _ = self.input.flush();
        let _ = self.child.wait();
    }
}

#[test]
fn sidecar_acknowledges_each_chunk_and_finishes_or_cancels() {
    let database_root = std::env::temp_dir().join(format!(
        "worlddb-ode-sidecar-test-{}-{}",
        std::process::id(),
        NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut sidecar = TestSidecar::start(&database_root);

    let transfer_id = "00000000000000000000000000000001";
    sidecar.send_request(&Request::StreamStart {
        transfer_id: transfer_id.to_owned(),
        total_bytes: 10,
        chunk_bytes: 4,
    });
    assert!(matches!(
        sidecar.read_response(),
        Response::StreamStarted { transfer_id: id } if id == transfer_id
    ));

    for (sequence, (chunk, expected_bytes)) in [
        (&[1_u8; 4][..], 4),
        (&[2_u8; 4][..], 8),
        (&[3_u8; 2][..], 10),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            sidecar.push_chunk(transfer_id, sequence as u64, chunk),
            Response::StreamChunkAccepted { sequence: accepted_sequence, bytes_received, .. }
                if accepted_sequence == sequence as u64 && bytes_received == expected_bytes
        ));
    }
    sidecar.send_request(&Request::StreamFinish {
        transfer_id: transfer_id.to_owned(),
        cancelled: false,
    });
    assert!(matches!(
        sidecar.read_response(),
        Response::StreamComplete {
            bytes_read: 10,
            cancelled: false,
            ..
        }
    ));

    let cancelled_id = "00000000000000000000000000000002";
    sidecar.send_request(&Request::StreamStart {
        transfer_id: cancelled_id.to_owned(),
        total_bytes: 10,
        chunk_bytes: 4,
    });
    assert!(matches!(
        sidecar.read_response(),
        Response::StreamStarted { .. }
    ));
    assert!(matches!(
        sidecar.push_chunk(cancelled_id, 0, &[9; 3]),
        Response::StreamChunkAccepted {
            bytes_received: 3,
            ..
        }
    ));
    sidecar.send_request(&Request::StreamFinish {
        transfer_id: cancelled_id.to_owned(),
        cancelled: true,
    });
    assert!(matches!(
        sidecar.read_response(),
        Response::StreamComplete {
            bytes_read: 3,
            cancelled: true,
            ..
        }
    ));

    sidecar.send_request(&Request::Shutdown);
    assert!(matches!(sidecar.read_response(), Response::Shutdown));
    let status = sidecar.child.wait().expect("sidecar exits");
    assert!(status.success());
    drop(sidecar);
    let _ = std::fs::remove_dir_all(database_root);
}
