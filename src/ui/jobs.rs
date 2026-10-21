//! Running an external command under a message panel with a spinner; Ctrl-C
//! pauses it and asks whether to stop. Port of lfm's `ProcessCommand`.

use std::ffi::OsStr;
use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use std::time::{Duration, Instant};

use crate::config::theme::{ColorItem, Theme};
use crate::jobs::Job;
use crate::keys::KeyCode;
use crate::ui::dialogs::{centered, footer, frame, ConfirmDialog};
use crate::ui::draw::{put, style};
use crate::ui::screen::{redraw, run_modal, Input, Screen};
use crate::ui::state::App;
use crate::ui::text::width;

#[derive(Debug)]
pub struct JobResult {
    pub stopped: bool,
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub struct MessagePanel {
    pub title: String,
    pub subtitle: String,
    pub spin: usize,
}

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

impl MessagePanel {
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let w = (width(&self.title).max(width(&self.subtitle)).max(22) + 6)
            .min(area.width.saturating_sub(2) as usize);
        let r = centered(area, w, 5);
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 2,
            &self.subtitle,
            style(theme, ColorItem::Dialog),
        );
        footer(
            buf,
            r,
            " Press Ctrl-C to stop ",
            style(theme, ColorItem::Dialog),
        );
        if area.width >= 3 {
            let spin = SPINNER[self.spin % SPINNER.len()];
            put(
                buf,
                area,
                area.right() - 3,
                area.y,
                spin,
                style(theme, ColorItem::DialogTitle),
            );
        }
    }
}

fn is_ctrl_c(i: &Input) -> bool {
    matches!(i, Input::Key(k) if k.ctrl && k.code == KeyCode::Char('c'))
}

/// Runs `cmd` (`sh -c`) in `cwd` while showing `title`/`subtitle`; Ctrl-C asks
/// whether to stop it. Spawn failures come back as `stderr`.
pub fn run_job(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    subtitle: &str,
    cmd: &OsStr,
    cwd: &Path,
) -> std::io::Result<JobResult> {
    let mut job = match Job::spawn(cmd, cwd) {
        Ok(j) => j,
        Err(e) => {
            return Ok(JobResult {
                stopped: false,
                status: None,
                stdout: String::new(),
                stderr: e.to_string(),
            })
        }
    };
    let mut panel = MessagePanel {
        title: title.into(),
        subtitle: subtitle.into(),
        spin: 0,
    };
    loop {
        // Draw first, so even a command that ends at once shows what ran.
        {
            let (p, a) = (&panel, &*app);
            redraw(
                screen,
                a,
                Some(&|area, buf| {
                    p.render(area, buf, &a.theme);
                    None
                }),
            )?;
        }
        if job.try_wait()?.is_some() {
            let out = job.finish();
            return Ok(JobResult {
                stopped: false,
                status: out.status,
                stdout: out.stdout,
                stderr: out.stderr,
            });
        }
        panel.spin += 1;
        let started = Instant::now();
        match screen.poll_input(TICK)? {
            Some(i) if is_ctrl_c(&i) => {
                job.pause()?;
                let question = format!("{title} {subtitle}");
                if run_modal(
                    screen,
                    app,
                    ConfirmDialog::new("Stop process", &question, false),
                )? {
                    job.kill()?;
                    let out = job.finish();
                    return Ok(JobResult {
                        stopped: true,
                        status: None,
                        stdout: out.stdout,
                        stderr: out.stderr,
                    });
                }
                job.resume()?;
            }
            Some(Input::Resize(w, h)) => app.resize(w, h),
            _ => {}
        }
        // A poll that returns at once (test screen) must not turn this into a busy loop.
        if let Some(rest) = TICK.checked_sub(started.elapsed()) {
            std::thread::sleep(rest.min(Duration::from_millis(10)));
        }
    }
}

const TICK: Duration = Duration::from_millis(50);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::screen::TestScreen;
    use crate::ui::testutil::test_app;

    #[test]
    fn job_completes_and_captures() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "");
        let r = run_job(
            &mut s,
            &mut app,
            "Find files",
            "x",
            OsStr::new("echo hi; echo err >&2; exit 2"),
            t.path(),
        )
        .unwrap();
        assert_eq!(
            (r.stopped, r.status, r.stdout.as_str(), r.stderr.as_str()),
            (false, Some(2), "hi\n", "err\n")
        );
        assert!(s.screen_text().contains("Find files"));
    }

    #[test]
    fn ctrl_c_and_yes_stops() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "C-c y");
        let start = std::time::Instant::now();
        let r = run_job(
            &mut s,
            &mut app,
            "Creating VFS",
            "a.tgz",
            OsStr::new("sleep 20"),
            t.path(),
        )
        .unwrap();
        assert!(r.stopped && start.elapsed().as_secs() < 5);
    }

    #[test]
    fn ctrl_c_and_no_continues() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "C-c n");
        let r = run_job(
            &mut s,
            &mut app,
            "T",
            "S",
            OsStr::new("sleep 0.3; echo done"),
            t.path(),
        )
        .unwrap();
        assert!(!r.stopped && r.stdout == "done\n");
    }

    #[test]
    fn panel_renders_in_small_areas() {
        let theme = Theme::default_theme();
        for (w, h) in [(80u16, 24u16), (10, 3), (1, 1)] {
            let r = Rect::new(0, 0, w, h);
            let mut b = Buffer::empty(r);
            MessagePanel {
                title: "Creating VFS".into(),
                subtitle: "a.tar.gz".into(),
                spin: 3,
            }
            .render(r, &mut b, &theme);
        }
    }
}
