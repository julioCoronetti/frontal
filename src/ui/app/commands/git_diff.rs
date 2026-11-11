use crate::ui::app::App;

impl App {
    pub fn load_git_diff() -> String {
        for args in [["diff"].as_slice(), ["diff", "HEAD"].as_slice()] {
            if let Ok(out) = std::process::Command::new("git").args(args).output() {
                if !out.stdout.is_empty() {
                    return String::from_utf8_lossy(&out.stdout).to_string();
                }
            }
        }
        String::from("diff --git (working tree clean)\nNo unstaged or uncommitted changes found.\n")
    }
}
