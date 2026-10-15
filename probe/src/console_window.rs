#[cfg(windows)]
pub fn wait_for_enter_if_started_from_explorer() {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    const ONLY_THIS_PROCESS: u32 = 1;

    let mut process_ids = [0_u32; 2];
    let attached_processes = unsafe { GetConsoleProcessList(process_ids.as_mut_ptr(), process_ids.len() as u32) };
    if attached_processes != ONLY_THIS_PROCESS {
        return;
    }
    println!("\nPress Enter to close this window.");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

#[cfg(not(windows))]
pub fn wait_for_enter_if_started_from_explorer() {}
