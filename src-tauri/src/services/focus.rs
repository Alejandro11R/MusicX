use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch, RwLock};

use crate::services::orchestrator::PlayerOrchestrator;

pub const DEFAULT_FOCUS_SECS: u32 = 25 * 60;
pub const DEFAULT_SHORT_BREAK_SECS: u32 = 5 * 60;
pub const DEFAULT_LONG_BREAK_SECS: u32 = 15 * 60;
pub const SESSIONS_UNTIL_LONG_BREAK: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FocusPhase {
    #[default]
    Idle,
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusState {
    pub phase: FocusPhase,
    pub remaining_seconds: u32,
    pub total_seconds: u32,
    pub is_running: bool,
    pub sessions_completed: u32,
    pub auto_pause_on_break: bool,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            phase: FocusPhase::Idle,
            remaining_seconds: DEFAULT_FOCUS_SECS,
            total_seconds: DEFAULT_FOCUS_SECS,
            is_running: false,
            sessions_completed: 0,
            auto_pause_on_break: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum FocusEvent {
    PhaseStarted { phase: FocusPhase, total_seconds: u32 },
    PhaseEnded { phase: FocusPhase, next_phase: FocusPhase },
    StateUpdated(FocusState),
}

pub struct FocusEngine {
    state: RwLock<FocusState>,
    deadline: RwLock<Option<Instant>>,
    orchestrator: Arc<PlayerOrchestrator>,
    state_tx: watch::Sender<FocusState>,
    event_tx: broadcast::Sender<FocusEvent>,
    auto_pause: AtomicBool,
}

impl FocusEngine {
    pub fn new(orchestrator: Arc<PlayerOrchestrator>) -> Arc<Self> {
        let initial_state = FocusState::default();
        let (state_tx, _) = watch::channel(initial_state.clone());
        let (event_tx, _) = broadcast::channel(32);

        let engine = Arc::new(Self {
            state: RwLock::new(initial_state),
            deadline: RwLock::new(None),
            orchestrator,
            state_tx,
            event_tx,
            auto_pause: AtomicBool::new(true),
        });

        let worker = engine.clone();
        tokio::spawn(async move {
            worker.run_ticker().await;
        });

        engine
    }

    pub async fn get_state(&self) -> FocusState {
        self.state.read().await.clone()
    }

    pub fn subscribe_state(&self) -> watch::Receiver<FocusState> {
        self.state_tx.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<FocusEvent> {
        self.event_tx.subscribe()
    }

    pub async fn start(&self) {
        let mut state = self.state.write().await;
        if state.is_running {
            return;
        }

        if state.phase == FocusPhase::Idle {
            state.phase = FocusPhase::Focus;
            state.remaining_seconds = DEFAULT_FOCUS_SECS;
            state.total_seconds = DEFAULT_FOCUS_SECS;
        }

        state.is_running = true;
        let mut deadline = self.deadline.write().await;
        *deadline = Some(Instant::now() + Duration::from_secs(state.remaining_seconds as u64));

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::PhaseStarted {
            phase: state.phase,
            total_seconds: state.total_seconds,
        });
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn pause(&self) {
        let mut state = self.state.write().await;
        if !state.is_running {
            return;
        }

        let mut deadline = self.deadline.write().await;
        if let Some(target) = *deadline {
            let now = Instant::now();
            if target > now {
                state.remaining_seconds = target.duration_since(now).as_secs() as u32;
            } else {
                state.remaining_seconds = 0;
            }
        }
        *deadline = None;
        state.is_running = false;

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        let mut deadline = self.deadline.write().await;
        *deadline = None;

        *state = FocusState {
            phase: FocusPhase::Idle,
            remaining_seconds: DEFAULT_FOCUS_SECS,
            total_seconds: DEFAULT_FOCUS_SECS,
            is_running: false,
            sessions_completed: state.sessions_completed,
            auto_pause_on_break: self.auto_pause.load(Ordering::SeqCst),
        };

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn skip(&self) {
        let current_phase = self.state.read().await.phase;
        self.advance_phase(current_phase).await;
    }

    pub async fn set_auto_pause(&self, enabled: bool) {
        self.auto_pause.store(enabled, Ordering::SeqCst);
        let mut state = self.state.write().await;
        state.auto_pause_on_break = enabled;
        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    async fn advance_phase(&self, finished_phase: FocusPhase) {
        let mut state = self.state.write().await;
        let next_phase = match finished_phase {
            FocusPhase::Focus | FocusPhase::Idle => {
                state.sessions_completed += 1;
                if state.sessions_completed % SESSIONS_UNTIL_LONG_BREAK == 0 {
                    FocusPhase::LongBreak
                } else {
                    FocusPhase::ShortBreak
                }
            }
            FocusPhase::ShortBreak | FocusPhase::LongBreak => FocusPhase::Focus,
        };

        let (total_secs, should_pause_audio) = match next_phase {
            FocusPhase::Focus => (DEFAULT_FOCUS_SECS, false),
            FocusPhase::ShortBreak => (DEFAULT_SHORT_BREAK_SECS, true),
            FocusPhase::LongBreak => (DEFAULT_LONG_BREAK_SECS, true),
            FocusPhase::Idle => (DEFAULT_FOCUS_SECS, false),
        };

        state.phase = next_phase;
        state.total_seconds = total_secs;
        state.remaining_seconds = total_secs;

        let mut deadline = self.deadline.write().await;
        if state.is_running {
            *deadline = Some(Instant::now() + Duration::from_secs(total_secs as u64));
        } else {
            *deadline = None;
        }

        let _ = self.event_tx.send(FocusEvent::PhaseEnded {
            phase: finished_phase,
            next_phase,
        });

        if should_pause_audio && self.auto_pause.load(Ordering::SeqCst) {
            tracing::info!("Focus session finished: auto-pausing playback for break");
            let _ = self.orchestrator.pause().await;
        }

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::PhaseStarted {
            phase: next_phase,
            total_seconds: total_secs,
        });
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    async fn run_ticker(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            let mut phase_to_advance: Option<FocusPhase> = None;

            {
                let mut state = self.state.write().await;
                if !state.is_running {
                    continue;
                }

                let deadline_guard = self.deadline.read().await;
                if let Some(target) = *deadline_guard {
                    let now = Instant::now();
                    if now >= target {
                        state.remaining_seconds = 0;
                        phase_to_advance = Some(state.phase);
                    } else {
                        state.remaining_seconds = target.duration_since(now).as_secs() as u32;
                    }
                }

                let _ = self.state_tx.send(state.clone());
                let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
            }

            if let Some(phase) = phase_to_advance {
                self.advance_phase(phase).await;
            }
        }
    }
}
