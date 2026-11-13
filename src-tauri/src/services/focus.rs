use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch, Mutex, RwLock};

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
    PhaseStarted {
        phase: FocusPhase,
        total_seconds: u32,
    },
    PhaseEnded {
        phase: FocusPhase,
        next_phase: FocusPhase,
    },
    StateUpdated(FocusState),
}

pub struct FocusEngine {
    // Serializes config, phase/deadline changes, events and playback effects.
    transition: Mutex<()>,
    state: RwLock<FocusState>,
    deadline: RwLock<Option<Instant>>,
    orchestrator: Arc<PlayerOrchestrator>,
    state_tx: watch::Sender<FocusState>,
    event_tx: broadcast::Sender<FocusEvent>,
    auto_pause: AtomicBool,
    focus_secs: std::sync::atomic::AtomicU32,
    short_break_secs: std::sync::atomic::AtomicU32,
    long_break_secs: std::sync::atomic::AtomicU32,
}

impl FocusEngine {
    pub fn new(orchestrator: Arc<PlayerOrchestrator>) -> Arc<Self> {
        let engine = Self::without_ticker(orchestrator);
        let worker = engine.clone();
        tauri::async_runtime::spawn(async move {
            worker.run_ticker().await;
        });
        engine
    }

    pub(crate) fn without_ticker(orchestrator: Arc<PlayerOrchestrator>) -> Arc<Self> {
        let initial_state = FocusState::default();
        let (state_tx, _) = watch::channel(initial_state.clone());
        let (event_tx, _) = broadcast::channel(32);

        let engine = Arc::new(Self {
            transition: Mutex::new(()),
            state: RwLock::new(initial_state),
            deadline: RwLock::new(None),
            orchestrator,
            state_tx,
            event_tx,
            auto_pause: AtomicBool::new(true),
            focus_secs: std::sync::atomic::AtomicU32::new(DEFAULT_FOCUS_SECS),
            short_break_secs: std::sync::atomic::AtomicU32::new(DEFAULT_SHORT_BREAK_SECS),
            long_break_secs: std::sync::atomic::AtomicU32::new(DEFAULT_LONG_BREAK_SECS),
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
        self.start_at(None).await;
    }

    async fn start_at(&self, now: Option<Instant>) {
        let _transition = self.transition.lock().await;
        let now = now.unwrap_or_else(Instant::now);
        let mut state = self.state.write().await;
        if state.is_running {
            return;
        }

        let focus_secs = self.focus_secs.load(Ordering::SeqCst);
        if state.phase == FocusPhase::Idle {
            state.phase = FocusPhase::Focus;
            state.remaining_seconds = focus_secs;
            state.total_seconds = focus_secs;
        }

        state.is_running = true;
        let mut deadline = self.deadline.write().await;
        *deadline = Some(now + Duration::from_secs(state.remaining_seconds as u64));

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::PhaseStarted {
            phase: state.phase,
            total_seconds: state.total_seconds,
        });
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn pause(&self) {
        self.pause_at(None).await;
    }

    async fn pause_at(&self, now: Option<Instant>) {
        let _transition = self.transition.lock().await;
        let now = now.unwrap_or_else(Instant::now);
        let mut state = self.state.write().await;
        if !state.is_running {
            return;
        }

        let mut deadline = self.deadline.write().await;
        if let Some(target) = *deadline {
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
        let _transition = self.transition.lock().await;
        let mut state = self.state.write().await;
        let mut deadline = self.deadline.write().await;
        *deadline = None;

        let focus_secs = self.focus_secs.load(Ordering::SeqCst);
        *state = FocusState {
            phase: FocusPhase::Idle,
            remaining_seconds: focus_secs,
            total_seconds: focus_secs,
            is_running: false,
            sessions_completed: state.sessions_completed,
            auto_pause_on_break: self.auto_pause.load(Ordering::SeqCst),
        };

        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn skip(&self) {
        let _transition = self.transition.lock().await;
        self.advance_phase(Instant::now()).await;
    }

    pub async fn set_auto_pause(&self, enabled: bool) {
        let _transition = self.transition.lock().await;
        self.auto_pause.store(enabled, Ordering::SeqCst);
        let mut state = self.state.write().await;
        state.auto_pause_on_break = enabled;
        let _ = self.state_tx.send(state.clone());
        let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
    }

    pub async fn set_durations(
        &self,
        focus_secs: u32,
        short_break_secs: u32,
        long_break_secs: u32,
    ) {
        let _transition = self.transition.lock().await;
        let focus_secs = focus_secs.clamp(60, 180 * 60);
        let short_break_secs = short_break_secs.clamp(30, 60 * 60);
        let long_break_secs = long_break_secs.clamp(60, 120 * 60);

        self.focus_secs.store(focus_secs, Ordering::SeqCst);
        self.short_break_secs
            .store(short_break_secs, Ordering::SeqCst);
        self.long_break_secs
            .store(long_break_secs, Ordering::SeqCst);

        let mut state = self.state.write().await;
        if state.phase == FocusPhase::Idle {
            state.remaining_seconds = focus_secs;
            state.total_seconds = focus_secs;
            let _ = self.state_tx.send(state.clone());
            let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
        }
    }

    // Caller holds transition through event publication and audio pause.
    async fn advance_phase(&self, now: Instant) {
        let mut state = self.state.write().await;
        let finished_phase = state.phase;
        let next_phase = match finished_phase {
            FocusPhase::Idle => return,
            FocusPhase::Focus => {
                state.sessions_completed += 1;
                if state.sessions_completed % SESSIONS_UNTIL_LONG_BREAK == 0 {
                    FocusPhase::LongBreak
                } else {
                    FocusPhase::ShortBreak
                }
            }
            FocusPhase::ShortBreak | FocusPhase::LongBreak => FocusPhase::Focus,
        };

        let focus_secs = self.focus_secs.load(Ordering::SeqCst);
        let short_break_secs = self.short_break_secs.load(Ordering::SeqCst);
        let long_break_secs = self.long_break_secs.load(Ordering::SeqCst);

        let (total_secs, should_pause_audio) = match next_phase {
            FocusPhase::Focus => (focus_secs, false),
            FocusPhase::ShortBreak => (short_break_secs, true),
            FocusPhase::LongBreak => (long_break_secs, true),
            FocusPhase::Idle => (focus_secs, false),
        };

        state.phase = next_phase;
        state.total_seconds = total_secs;
        state.remaining_seconds = total_secs;

        let mut deadline = self.deadline.write().await;
        if state.is_running {
            *deadline = Some(now + Duration::from_secs(total_secs as u64));
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

            self.tick_at(None).await;
        }
    }

    async fn tick_at(&self, now: Option<Instant>) {
        let _transition = self.transition.lock().await;
        let now = now.unwrap_or_else(Instant::now);
        let mut state = self.state.write().await;
        if !state.is_running {
            return;
        }
        let deadline = *self.deadline.read().await;
        if let Some(target) = deadline {
            if now >= target {
                drop(state);
                self.advance_phase(now).await;
                return;
            }
            state.remaining_seconds = target.duration_since(now).as_secs() as u32;
            let _ = self.state_tx.send(state.clone());
            let _ = self.event_tx.send(FocusEvent::StateUpdated(state.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        mpv_event::MpvPlayer,
        orchestrator::{PlaybackController, TrackResolver},
        yt_dlp::YtDlpService,
    };

    fn fixture() -> Arc<FocusEngine> {
        let mpv = Arc::new(MpvPlayer::fake());
        FocusEngine::without_ticker(Arc::new(PlayerOrchestrator::new(
            PlaybackController::new(mpv.clone()),
            TrackResolver::new(Arc::new(YtDlpService::new())),
            mpv.subscribe_events(),
            mpv.subscribe_status(),
            None,
        )))
    }

    #[tokio::test]
    async fn idle_skip_is_silent_and_unchanged() {
        let engine = fixture();
        let mut events = engine.subscribe_events();
        let before = engine.get_state().await;
        engine.skip().await;
        assert_eq!(engine.get_state().await, before);
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn deadlines_resume_and_reset_preserve_existing_behavior() {
        let engine = fixture();
        let now = Instant::now();
        engine.set_durations(120, 30, 60).await;
        engine.set_auto_pause(false).await;
        engine.start_at(Some(now)).await;
        let mut events = engine.subscribe_events();
        engine.start_at(Some(now + Duration::from_secs(10))).await;
        assert!(events.try_recv().is_err(), "Start is idempotent");
        engine.tick_at(Some(now + Duration::from_secs(20))).await;
        assert_eq!(engine.get_state().await.remaining_seconds, 100);
        engine.pause_at(Some(now + Duration::from_secs(25))).await;
        assert_eq!(engine.get_state().await.remaining_seconds, 95);
        engine.tick_at(Some(now + Duration::from_secs(1000))).await;
        assert_eq!(engine.get_state().await.remaining_seconds, 95);
        engine.start_at(Some(now + Duration::from_secs(1000))).await;
        engine.tick_at(Some(now + Duration::from_secs(1094))).await;
        assert_eq!(engine.get_state().await.remaining_seconds, 1);
        engine.tick_at(Some(now + Duration::from_secs(1095))).await;
        let state = engine.get_state().await;
        assert_eq!(state.phase, FocusPhase::ShortBreak);
        assert_eq!(state.sessions_completed, 1);
        engine.set_durations(180, 90, 120).await;
        engine.tick_at(Some(now + Duration::from_secs(1125))).await;
        assert_eq!(engine.get_state().await.phase, FocusPhase::Focus);
        assert_eq!(engine.get_state().await.total_seconds, 180);
        assert_eq!(engine.get_state().await.sessions_completed, 1);
        engine.reset().await;
        let state = engine.get_state().await;
        assert_eq!(state.phase, FocusPhase::Idle);
        assert_eq!(state.sessions_completed, 1);
        assert_eq!(state.remaining_seconds, 180);
        assert!(!state.is_running);
    }

    #[tokio::test]
    async fn break_cadence_and_audio_commands_remain_unchanged() {
        for auto_pause in [false, true] {
            let (mpv, mut commands) = MpvPlayer::controlled_fake();
            let mpv = Arc::new(mpv);
            let engine = FocusEngine::without_ticker(Arc::new(PlayerOrchestrator::new(
                PlaybackController::new(mpv.clone()),
                TrackResolver::new(Arc::new(YtDlpService::new())),
                mpv.subscribe_events(),
                mpv.subscribe_status(),
                None,
            )));
            engine.set_auto_pause(auto_pause).await;
            engine.skip().await;
            assert!(commands.try_recv().is_err());
            engine.start().await;
            for session in 1..=4 {
                let advance = engine.skip();
                if auto_pause {
                    let acknowledge = async {
                        let (name, reply) = commands.recv().await.unwrap();
                        assert_eq!(name, "pause");
                        reply.send(Ok(())).unwrap();
                    };
                    tokio::join!(advance, acknowledge);
                } else {
                    advance.await;
                }
                let state = engine.get_state().await;
                assert_eq!(state.sessions_completed, session);
                assert_eq!(
                    state.phase,
                    if session == 4 {
                        FocusPhase::LongBreak
                    } else {
                        FocusPhase::ShortBreak
                    }
                );
                engine.skip().await;
                assert_eq!(engine.get_state().await.phase, FocusPhase::Focus);
                assert_eq!(engine.get_state().await.sessions_completed, session);
                assert!(
                    commands.try_recv().is_err(),
                    "No automatic resume or disabled pause"
                );
            }
        }
    }

    #[tokio::test]
    async fn pending_audio_pause_serializes_settings_and_expiry() {
        use std::{future::Future, task::Poll};
        let (mpv, mut commands) = MpvPlayer::controlled_fake();
        let mpv = Arc::new(mpv);
        let engine = FocusEngine::without_ticker(Arc::new(PlayerOrchestrator::new(
            PlaybackController::new(mpv.clone()),
            TrackResolver::new(Arc::new(YtDlpService::new())),
            mpv.subscribe_events(),
            mpv.subscribe_status(),
            None,
        )));
        engine.start().await;
        let mut events = engine.subscribe_events();
        let advance = engine.skip();
        let overlap = async {
            let (name, reply) = commands.recv().await.unwrap();
            assert_eq!(name, "pause");
            let settings = engine.set_durations(120, 90, 180);
            let expiry = engine.tick_at(Some(Instant::now() + Duration::from_secs(1000)));
            tokio::pin!(settings, expiry);
            std::future::poll_fn(|cx| {
                assert!(settings.as_mut().poll(cx).is_pending());
                assert!(expiry.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            reply.send(Ok(())).unwrap();
            settings.await;
            expiry.await;
        };
        tokio::join!(advance, overlap);
        let state = engine.get_state().await;
        assert_eq!(state.phase, FocusPhase::Focus);
        assert_eq!(state.sessions_completed, 1);
        assert_eq!(state.total_seconds, 120);
        assert!(commands.try_recv().is_err());
        let mut ended = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let FocusEvent::PhaseEnded { phase, next_phase } = event {
                ended.push((phase, next_phase));
            }
        }
        assert_eq!(
            ended,
            vec![
                (FocusPhase::Focus, FocusPhase::ShortBreak),
                (FocusPhase::ShortBreak, FocusPhase::Focus)
            ]
        );
    }

    #[tokio::test]
    async fn configuration_preserves_each_running_and_paused_interval() {
        for running in [true, false] {
            for skips in [0, 1, 7] {
                let engine = fixture();
                engine.set_auto_pause(false).await;
                let now = Instant::now();
                engine.start_at(Some(now)).await;
                for _ in 0..skips {
                    engine.skip().await;
                }
                if !running {
                    engine.pause_at(Some(now)).await;
                }
                let before = engine.get_state().await;
                engine.set_durations(120, 90, 180).await;
                assert_eq!(engine.get_state().await, before);
                engine.skip().await;
                let next = engine.get_state().await;
                assert_eq!(next.total_seconds, if skips == 0 { 90 } else { 120 });
                assert_eq!(next.is_running, running);
            }
        }
    }
}
