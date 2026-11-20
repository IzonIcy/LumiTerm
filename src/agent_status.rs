//! Which coding agent, if any, is running inside a tab's shell.
//!
//! Detection is a process-tree walk over one `ps` snapshot, so it needs no
//! shell hooks, no environment variables, and no cooperation from the agent.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Executable basenames treated as coding agents.
pub const AGENT_NAMES: [&str; 6] = ["aider", "claude", "codex", "droid", "gemini", "opencode"];

/// How often the background scanner takes a fresh `ps` snapshot.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// An agent that has produced no terminal output for this long is Idle
/// rather than Running.
pub const IDLE_AFTER: Duration = Duration::from_secs(10);

/// An agent that ran for less than this is not worth a notification.
pub const NOTIFY_MIN_RUN: Duration = Duration::from_secs(10);

/// A recognised agent process found under a tab's shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentProcess {
    pub pid: u32,
    pub name: &'static str,
}

/// One row of `ps -axo pid,ppid,comm`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
}

/// Parses the `ps` table. The header line drops out on its own because "PID"
/// is not a number, and `comm` is kept whole so a name with spaces in it still
/// compares against the agent list.
pub fn parse_ps_table(output: &str) -> Vec<ProcessEntry> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let comm = fields.collect::<Vec<_>>().join(" ");
            if comm.is_empty() {
                return None;
            }
            Some(ProcessEntry { pid, ppid, comm })
        })
        .collect()
}

/// The agent name for an executable path, matched on the basename only.
pub fn agent_name(comm: &str) -> Option<&'static str> {
    let basename = comm.rsplit('/').next()?;
    AGENT_NAMES.iter().copied().find(|name| *name == basename)
}

/// The nearest agent process below `root`, breadth-first. Processes outside
/// that tree belong to other tabs and are never returned.
pub fn find_agent_descendant(entries: &[ProcessEntry], root: u32) -> Option<AgentProcess> {
    let mut children: HashMap<u32, Vec<&ProcessEntry>> = HashMap::new();
    for entry in entries {
        children.entry(entry.ppid).or_default().push(entry);
    }

    let mut queue = VecDeque::from([root]);
    // A torn `ps` snapshot can report a pid as its own ancestor; without this
    // the walk would spin instead of returning.
    let mut visited = HashSet::from([root]);
    while let Some(pid) = queue.pop_front() {
        for child in children.get(&pid).map(Vec::as_slice).unwrap_or_default() {
            if let Some(name) = agent_name(&child.comm) {
                return Some(AgentProcess {
                    pid: child.pid,
                    name,
                });
            }
            if visited.insert(child.pid) {
                queue.push_back(child.pid);
            }
        }
    }
    None
}

/// What the sidebar shows for one tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    /// No agent process under this tab's shell.
    None,
    /// An agent process is alive and the tab produced output recently.
    Running,
    /// An agent process is alive but the tab has been quiet for `IDLE_AFTER`.
    Idle,
    /// An agent this tab had running is gone.
    Exited,
}

/// An agent that disappeared, and how long this app watched it run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentExit {
    pub name: &'static str,
    pub ran_for: Duration,
}

impl AgentExit {
    pub fn should_notify(&self) -> bool {
        self.ran_for >= NOTIFY_MIN_RUN
    }

    pub fn notification_body(&self, tab_title: &str) -> String {
        format!(
            "{} exited in {tab_title} after {}s",
            self.name,
            self.ran_for.as_secs()
        )
    }
}

/// Per-tab agent state. Holds the last scan's observation and derives the
/// visible state from it plus how long the tab has been silent, so an agent
/// that goes quiet flips to Idle without waiting for the next scan.
#[derive(Debug, Default)]
pub struct AgentStatus {
    observed: Option<AgentProcess>,
    seen_at: Option<Instant>,
    exited: bool,
}

impl AgentStatus {
    /// Folds in one scan result. Returns an exit when the agent this status
    /// was tracking is gone; a scan that merely repeats `None` returns nothing.
    pub fn record(&mut self, observed: Option<AgentProcess>, now: Instant) -> Option<AgentExit> {
        match observed {
            Some(agent) => {
                if self.observed.map(|known| known.pid) != Some(agent.pid) {
                    self.seen_at = Some(now);
                }
                self.observed = Some(agent);
                self.exited = false;
                None
            }
            None => {
                let previous = self.observed.take()?;
                self.exited = true;
                let started = self.seen_at.take().unwrap_or(now);
                Some(AgentExit {
                    name: previous.name,
                    ran_for: now.saturating_duration_since(started),
                })
            }
        }
    }

    pub fn state(&self, output_age: Duration) -> AgentState {
        match self.observed {
            Some(_) if output_age < IDLE_AFTER => AgentState::Running,
            Some(_) => AgentState::Idle,
            None if self.exited => AgentState::Exited,
            None => AgentState::None,
        }
    }

    pub fn agent_name(&self) -> Option<&'static str> {
        self.observed.map(|agent| agent.name)
    }
}

/// One tab the scanner should watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchTarget {
    pub id: u64,
    pub shell_pid: Option<u32>,
}

/// One tab's scan result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentObservation {
    pub id: u64,
    pub agent: Option<AgentProcess>,
}

/// Runs the process scan on a worker thread and hands results back through a
/// channel, so no `ps` spawn, `wait`, or parse ever touches the render thread.
pub struct AgentMonitor {
    watch_tx: Option<Sender<Vec<WatchTarget>>>,
    results_rx: Receiver<Vec<AgentObservation>>,
    last_targets: Vec<WatchTarget>,
    worker: Option<JoinHandle<()>>,
}

impl AgentMonitor {
    pub fn new() -> Self {
        let (watch_tx, watch_rx) = mpsc::channel();
        let (results_tx, results_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("lumi-term-agent-scan".to_owned())
            .spawn(move || watch_loop(watch_rx, results_tx))
            .ok();
        Self {
            watch_tx: Some(watch_tx),
            results_rx,
            last_targets: Vec::new(),
            worker,
        }
    }

    /// Publishes the tabs to watch. Unchanged lists cost one comparison and
    /// send nothing, so calling this every frame is fine.
    pub fn set_targets(&mut self, targets: Vec<WatchTarget>) {
        if targets == self.last_targets {
            return;
        }
        if let Some(watch_tx) = &self.watch_tx {
            let _ = watch_tx.send(targets.clone());
        }
        self.last_targets = targets;
    }

    /// The newest completed scan, if one finished since the last call. Never
    /// blocks.
    pub fn poll(&mut self) -> Vec<AgentObservation> {
        let mut latest = Vec::new();
        while let Ok(batch) = self.results_rx.try_recv() {
            latest = batch;
        }
        latest
    }
}

impl Drop for AgentMonitor {
    fn drop(&mut self) {
        // Dropping the sender wakes the worker out of recv_timeout so it exits
        // now instead of after another interval.
        self.watch_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn watch_loop(watch_rx: Receiver<Vec<WatchTarget>>, results_tx: Sender<Vec<AgentObservation>>) {
    let mut targets: Vec<WatchTarget> = Vec::new();
    loop {
        match watch_rx.recv_timeout(POLL_INTERVAL) {
            Ok(next) => targets = next,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if targets.is_empty() {
            continue;
        }
        if results_tx.send(scan(&targets)).is_err() {
            return;
        }
    }
}

fn scan(targets: &[WatchTarget]) -> Vec<AgentObservation> {
    // A failed `ps` yields no observations at all rather than a round of
    // `None`s, so a transient failure cannot look like every agent exited.
    let Some(table) = read_process_table() else {
        return Vec::new();
    };
    targets
        .iter()
        .map(|target| AgentObservation {
            id: target.id,
            agent: target
                .shell_pid
                .and_then(|pid| find_agent_descendant(&table, pid)),
        })
        .collect()
}

fn read_process_table() -> Option<Vec<ProcessEntry>> {
    let output = Command::new("ps")
        .args(["-axo", "pid,ppid,comm"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_ps_table(&String::from_utf8_lossy(&output.stdout)))
}

/// Posts a desktop notification. Spawned on a throwaway thread because the
/// command takes tens of milliseconds and this is called from the render
/// thread. Non-macOS platforms do nothing.
pub fn post_notification(body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"Lumi-Term\"",
            escape_applescript(body)
        );
        let _ = thread::Builder::new()
            .name("lumi-term-notifier".to_owned())
            .spawn(move || {
                let _ = Command::new("osascript")
                    .args(["-e", &script])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = body;
    }
}

#[cfg(target_os = "macos")]
fn escape_applescript(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS_FIXTURE: &str = "  PID  PPID COMM
    1     0 /sbin/launchd
   40     1 /System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow
  100     1 /bin/zsh
  200   100 zsh
  300   200 node
  400   300 /Users/ryan/.local/bin/claude
";

    const AGENT: AgentProcess = AgentProcess {
        pid: 400,
        name: "claude",
    };

    #[test]
    fn parses_the_ps_table_and_skips_the_header() {
        let table = parse_ps_table(PS_FIXTURE);
        assert_eq!(table.len(), 6);
        assert_eq!(
            table[0],
            ProcessEntry {
                pid: 1,
                ppid: 0,
                comm: "/sbin/launchd".to_owned(),
            }
        );
        assert_eq!(
            table[1].comm,
            "/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow",
            "a comm with no spaces survives intact"
        );
        assert_eq!(table[5].pid, 400);
        assert_eq!(table[5].ppid, 300);
    }

    #[test]
    fn matches_agents_by_executable_basename_only() {
        assert_eq!(agent_name("claude"), Some("claude"));
        assert_eq!(agent_name("/opt/homebrew/bin/codex"), Some("codex"));
        assert_eq!(agent_name("opencode"), Some("opencode"));
        assert_eq!(agent_name("gemini"), Some("gemini"));
        assert_eq!(agent_name("droid"), Some("droid"));
        assert_eq!(agent_name("aider"), Some("aider"));

        assert_eq!(agent_name("node"), None, "the interpreter is not the agent");
        assert_eq!(
            agent_name("claude-helper"),
            None,
            "basename must match whole"
        );
        assert_eq!(agent_name("/tmp/my-aider"), None);
        assert_eq!(agent_name("Claude"), None, "names are case-sensitive");
    }

    #[test]
    fn walks_a_multi_level_tree_from_the_shell_down() {
        let table = parse_ps_table(PS_FIXTURE);
        assert_eq!(find_agent_descendant(&table, 100), Some(AGENT));
        assert_eq!(
            find_agent_descendant(&table, 300),
            Some(AGENT),
            "an intermediate process is a valid root"
        );
        assert_eq!(
            find_agent_descendant(&table, 200),
            Some(AGENT),
            "three levels below the shell"
        );
        assert_eq!(
            find_agent_descendant(&table, 40),
            None,
            "loginwindow has no agent under it"
        );

        let sibling = parse_ps_table("100 1 zsh\n200 100 zsh\n700 999 codex\n");
        assert_eq!(
            find_agent_descendant(&sibling, 100),
            None,
            "an agent in another process tree is not this tab's"
        );

        let cycle = parse_ps_table("50 50 zsh\n60 60 bash\n");
        assert_eq!(
            find_agent_descendant(&cycle, 50),
            None,
            "a pid that parents itself must end the walk, not hang it"
        );
    }

    #[test]
    fn agent_moves_from_running_to_idle_to_exited() {
        let started = Instant::now();
        let mut status = AgentStatus::default();
        assert_eq!(status.state(Duration::ZERO), AgentState::None);

        assert_eq!(
            status.record(Some(AGENT), started),
            None,
            "appearing is not an exit"
        );
        assert_eq!(status.agent_name(), Some("claude"));
        assert_eq!(status.state(Duration::from_secs(1)), AgentState::Running);
        assert_eq!(
            status.state(IDLE_AFTER),
            AgentState::Idle,
            "a quiet agent is idle, not running"
        );
        assert_eq!(status.state(Duration::from_secs(60)), AgentState::Idle);

        let exit = status
            .record(None, started + Duration::from_secs(30))
            .expect("the tracked agent is gone");
        assert_eq!(exit.name, "claude");
        assert_eq!(exit.ran_for, Duration::from_secs(30));
        assert!(exit.should_notify());
        assert_eq!(status.agent_name(), None);
        assert_eq!(
            status.state(Duration::ZERO),
            AgentState::Exited,
            "exited latches until another agent appears"
        );

        assert_eq!(
            status.record(None, started + Duration::from_secs(40)),
            None,
            "a second empty scan must not report the same exit twice"
        );
        assert_eq!(status.state(Duration::ZERO), AgentState::Exited);

        let codex = AgentProcess {
            pid: 900,
            name: "codex",
        };
        status.record(Some(codex), started + Duration::from_secs(50));
        assert_eq!(status.state(Duration::ZERO), AgentState::Running);
        assert_eq!(status.agent_name(), Some("codex"));
    }

    #[test]
    fn a_new_agent_pid_restarts_the_run_clock() {
        let started = Instant::now();
        let mut status = AgentStatus::default();
        status.record(Some(AGENT), started);
        status.record(
            Some(AgentProcess {
                pid: 500,
                name: "aider",
            }),
            started + Duration::from_secs(5),
        );

        let exit = status
            .record(None, started + Duration::from_secs(8))
            .expect("aider is gone");
        assert_eq!(exit.name, "aider");
        assert_eq!(
            exit.ran_for,
            Duration::from_secs(3),
            "the clock starts at the current agent, not the first one ever seen"
        );
    }

    #[test]
    fn notifications_skip_short_agent_runs() {
        let quick = AgentExit {
            name: "aider",
            ran_for: NOTIFY_MIN_RUN - Duration::from_secs(1),
        };
        assert!(!quick.should_notify(), "a few seconds is not worth a toast");

        let long = AgentExit {
            name: "aider",
            ran_for: NOTIFY_MIN_RUN,
        };
        assert!(long.should_notify(), "the threshold is inclusive");
        assert_eq!(
            long.notification_body("Shell 2"),
            "aider exited in Shell 2 after 10s"
        );

        let barely_seen = AgentExit {
            name: "claude",
            ran_for: Duration::ZERO,
        };
        assert!(!barely_seen.should_notify());
    }
}
