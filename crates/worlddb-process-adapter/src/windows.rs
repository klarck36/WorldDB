use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::ptr::null;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

use crate::IsolatedChild;

pub(super) struct JobObject(OwnedHandle);

impl JobObject {
    fn as_raw(&self) -> HANDLE {
        self.0.as_raw_handle() as HANDLE
    }
}

pub(super) fn terminate_process_tree(job: &JobObject) -> io::Result<()> {
    // SAFETY: the job handle remains owned and open for the complete API call.
    // TEST: windows_resource_limits::dropping_or_finishing_kills_remaining_descendants.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let terminated = unsafe { TerminateJobObject(job.as_raw(), 1) };
    if terminated == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn spawn(command: &mut Command, memory_limit_bytes: u64) -> io::Result<IsolatedChild> {
    let memory_limit_bytes = usize::try_from(memory_limit_bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "process memory limit does not fit this platform",
        )
    })?;
    let job = create_limited_job(memory_limit_bytes)?;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let mut child = command.spawn()?;

    if let Err(error) = assign_child(&job, &child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    if let Err(error) = resume_primary_thread(child.id()) {
        drop(job);
        let _ = child.wait();
        return Err(error);
    }

    Ok(IsolatedChild { child, _job: job })
}

fn create_limited_job(memory_limit_bytes: usize) -> io::Result<JobObject> {
    // SAFETY: null security attributes and name create an anonymous job handle.
    // TEST: windows_resource_limits::memory_exhaustion_is_rejected_by_the_job_limit.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: CreateJobObjectW returned a valid, uniquely owned job handle.
    // TEST: windows_resource_limits::memory_exhaustion_is_rejected_by_the_job_limit.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let owned = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
    let job = JobObject(owned);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.BasicLimitInformation.ActiveProcessLimit = 64;
    limits.JobMemoryLimit = memory_limit_bytes;
    limits.ProcessMemoryLimit = memory_limit_bytes;
    let length = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(|_| io::Error::other("job limit structure size overflow"))?;

    // SAFETY: the initialized structure stays alive for the complete API call.
    // TEST: windows_resource_limits::memory_exhaustion_is_rejected_by_the_job_limit.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let applied = unsafe {
        SetInformationJobObject(
            job.as_raw(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            length,
        )
    };
    if applied == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn assign_child(job: &JobObject, child: &Child) -> io::Result<()> {
    // SAFETY: the process handle belongs to this live, suspended Child.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let assigned =
        unsafe { AssignProcessToJobObject(job.as_raw(), child.as_raw_handle() as HANDLE) };
    if assigned == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn resume_primary_thread(process_id: u32) -> io::Result<()> {
    let snapshot = create_thread_snapshot()?;
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>())
            .map_err(|_| io::Error::other("thread entry structure size overflow"))?,
        ..THREADENTRY32::default()
    };
    let mut found = first_thread(&snapshot, &mut entry);
    while found {
        if entry.th32OwnerProcessID == process_id {
            let thread = open_suspended_thread(entry.th32ThreadID)?;
            resume_thread(&thread)?;
            return Ok(());
        }
        entry.dwSize = u32::try_from(size_of::<THREADENTRY32>())
            .map_err(|_| io::Error::other("thread entry structure size overflow"))?;
        found = next_thread(&snapshot, &mut entry);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "suspended adapter primary thread was not found",
    ))
}

fn create_thread_snapshot() -> io::Result<OwnedHandle> {
    // SAFETY: flags request a fresh process-wide thread snapshot.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Toolhelp returned one valid owned snapshot handle.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let owned = unsafe { OwnedHandle::from_raw_handle(snapshot as RawHandle) };
    Ok(owned)
}

fn first_thread(snapshot: &OwnedHandle, entry: &mut THREADENTRY32) -> bool {
    // SAFETY: entry is initialized and writable; snapshot is an open thread snapshot.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let found = unsafe { Thread32First(snapshot.as_raw_handle() as HANDLE, entry) };
    found != 0
}

fn next_thread(snapshot: &OwnedHandle, entry: &mut THREADENTRY32) -> bool {
    // SAFETY: entry is initialized and writable; snapshot is an open thread snapshot.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let found = unsafe { Thread32Next(snapshot.as_raw_handle() as HANDLE, entry) };
    found != 0
}

fn open_suspended_thread(thread_id: u32) -> io::Result<OwnedHandle> {
    // SAFETY: the snapshot supplied the thread ID for the suspended child.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenThread returned a valid uniquely owned thread handle.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let owned = unsafe { OwnedHandle::from_raw_handle(thread as RawHandle) };
    Ok(owned)
}

fn resume_thread(thread: &OwnedHandle) -> io::Result<()> {
    // SAFETY: thread is open with THREAD_SUSPEND_RESUME access.
    // TEST: windows_resource_limits::child_is_assigned_before_its_first_instruction.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0005")]
    let previous_suspend_count = unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) };
    if previous_suspend_count == u32::MAX {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
