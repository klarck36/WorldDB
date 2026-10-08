# M5-10 macOS/APFS adapter

## Policy

ODE-006 requires `F_FULLFSYNC` for durable regular-file writes on macOS. The
storage crate now routes regular-file sync operations through
`platform_sync::sync_file`: macOS calls `fcntl(F_FULLFSYNC)`, while other
platforms keep `File::sync_all()`. An APFS full-sync error is returned to the
caller without falling back to `fsync`, so a failed or unsupported operation
prevents the affected publication from being reported as durable.

Directory synchronization remains a separate operation. It opens the target
directory and calls `sync_all()`; the file full-sync call is never applied to a
directory descriptor. Publication and writer locking continue to use the
existing same-volume rename and operating-system lock paths, which require
macOS runner evidence before M5-10 can close.

## Verification status

Implementation is ready locally. The APFS runner checks and focused storage
tests have not yet completed for this change. The current M8-26b run was
started before this implementation and does not verify it.

M5-10 can close after a SHA-pinned macOS/APFS run verifies the new regular-file
full-sync path, injected sync-error propagation, directory sync, writer lock,
and publication behavior. Record its run and artifact hashes here and in the
task register. Passing runner tests establish behavior on that hosted APFS
profile only; they do not establish survival across power loss or generalize
to all Mac hardware. Those limits remain part of M5-23.
