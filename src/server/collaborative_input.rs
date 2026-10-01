use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const GESTURE_IDLE_TIMEOUT_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerAction {
    Move,
    Down(i32),
    Up(i32),
    Scroll,
    RelativeMove,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PointerDecision {
    pub simulate: bool,
    pub move_before_action: Option<(i32, i32)>,
    pub release_stale_buttons: Vec<i32>,
}

#[derive(Debug)]
struct GestureLease {
    owner: i32,
    pressed_buttons: BTreeSet<i32>,
    last_activity_ms: u64,
}

#[derive(Debug, Default)]
struct PointerArbiter {
    positions: HashMap<i32, (i32, i32)>,
    lease: Option<GestureLease>,
}

impl PointerArbiter {
    fn expire_stale_lease(&mut self, now_ms: u64) -> Vec<i32> {
        let expired = self
            .lease
            .as_ref()
            .map(|lease| now_ms.saturating_sub(lease.last_activity_ms) > GESTURE_IDLE_TIMEOUT_MS)
            .unwrap_or(false);
        if !expired {
            return Vec::new();
        }
        self.lease
            .take()
            .map(|lease| lease.pressed_buttons.into_iter().collect())
            .unwrap_or_default()
    }

    fn decide(
        &mut self,
        conn: i32,
        action: PointerAction,
        x: i32,
        y: i32,
        now_ms: u64,
    ) -> PointerDecision {
        let release_stale_buttons = self.expire_stale_lease(now_ms);
        if action == PointerAction::Move {
            self.positions.insert(conn, (x, y));
        }
        let position = self.positions.get(&conn).copied();

        let mut decision = PointerDecision {
            release_stale_buttons,
            ..Default::default()
        };
        match action {
            PointerAction::Move => {
                if let Some(lease) = self.lease.as_mut() {
                    if lease.owner == conn {
                        lease.last_activity_ms = now_ms;
                        decision.simulate = true;
                    }
                }
            }
            PointerAction::Down(button) => match self.lease.as_mut() {
                Some(lease) if lease.owner == conn => {
                    lease.pressed_buttons.insert(button);
                    lease.last_activity_ms = now_ms;
                    decision.simulate = true;
                    decision.move_before_action = position;
                }
                Some(_) => {}
                None => {
                    self.lease = Some(GestureLease {
                        owner: conn,
                        pressed_buttons: BTreeSet::from([button]),
                        last_activity_ms: now_ms,
                    });
                    decision.simulate = true;
                    decision.move_before_action = position;
                }
            },
            PointerAction::Up(button) => {
                if let Some(lease) = self.lease.as_mut() {
                    if lease.owner == conn && lease.pressed_buttons.remove(&button) {
                        lease.last_activity_ms = now_ms;
                        decision.simulate = true;
                        if lease.pressed_buttons.is_empty() {
                            self.lease = None;
                        }
                    }
                }
            }
            PointerAction::Scroll => match self.lease.as_mut() {
                Some(lease) if lease.owner == conn => {
                    lease.last_activity_ms = now_ms;
                    decision.simulate = true;
                    decision.move_before_action = position;
                }
                Some(_) => {}
                None => {
                    decision.simulate = true;
                    decision.move_before_action = position;
                }
            },
            PointerAction::RelativeMove => {
                // Hover remains presence-only. During a drag, the owner may
                // move relatively while keeping its button lease.
                if let Some(lease) = self.lease.as_mut() {
                    if lease.owner == conn {
                        lease.last_activity_ms = now_ms;
                        decision.simulate = true;
                    }
                }
            }
        }
        decision
    }

    fn clear_connection(&mut self, conn: i32) -> Vec<i32> {
        self.positions.remove(&conn);
        if self
            .lease
            .as_ref()
            .map(|lease| lease.owner == conn)
            .unwrap_or(false)
        {
            return self
                .lease
                .take()
                .map(|lease| lease.pressed_buttons.into_iter().collect())
                .unwrap_or_default();
        }
        Vec::new()
    }
}

static STARTED_AT: OnceLock<Instant> = OnceLock::new();
static POINTER_ARBITER: OnceLock<Mutex<PointerArbiter>> = OnceLock::new();

pub(crate) fn decide_pointer(conn: i32, action: PointerAction, x: i32, y: i32) -> PointerDecision {
    let now_ms = STARTED_AT
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    POINTER_ARBITER
        .get_or_init(|| Mutex::new(PointerArbiter::default()))
        .lock()
        .unwrap()
        .decide(conn, action, x, y, now_ms)
}

pub(crate) fn clear_connection(conn: i32) -> Vec<i32> {
    POINTER_ARBITER
        .get_or_init(|| Mutex::new(PointerArbiter::default()))
        .lock()
        .unwrap()
        .clear_connection(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_is_presence_only_and_click_moves_atomically() {
        let mut arbiter = PointerArbiter::default();
        assert_eq!(
            arbiter.decide(1, PointerAction::Move, 400, 300, 1),
            PointerDecision::default()
        );
        assert_eq!(
            arbiter.decide(1, PointerAction::Down(1), 0, 0, 2),
            PointerDecision {
                simulate: true,
                move_before_action: Some((400, 300)),
                release_stale_buttons: Vec::new(),
            }
        );
        assert!(arbiter.decide(1, PointerAction::Up(1), 0, 0, 3).simulate);
    }

    #[test]
    fn drag_owner_blocks_other_pointer_until_button_up() {
        let mut arbiter = PointerArbiter::default();
        arbiter.decide(1, PointerAction::Move, 10, 20, 1);
        arbiter.decide(2, PointerAction::Move, 900, 700, 1);
        assert!(arbiter.decide(1, PointerAction::Down(1), 0, 0, 2).simulate);
        assert!(arbiter.decide(1, PointerAction::Move, 30, 40, 3).simulate);
        assert!(!arbiter.decide(2, PointerAction::Down(1), 0, 0, 4).simulate);
        assert!(arbiter.decide(1, PointerAction::Up(1), 0, 0, 5).simulate);
        assert!(arbiter.decide(2, PointerAction::Down(1), 0, 0, 6).simulate);
    }

    #[test]
    fn relative_motion_is_allowed_only_for_the_drag_owner() {
        let mut arbiter = PointerArbiter::default();
        assert!(!arbiter
            .decide(1, PointerAction::RelativeMove, 4, 5, 1)
            .simulate);
        arbiter.decide(1, PointerAction::Down(1), 0, 0, 2);
        assert!(arbiter
            .decide(1, PointerAction::RelativeMove, 4, 5, 3)
            .simulate);
        assert!(!arbiter
            .decide(2, PointerAction::RelativeMove, 4, 5, 4)
            .simulate);
        arbiter.decide(1, PointerAction::Up(1), 0, 0, 5);
        assert!(!arbiter
            .decide(1, PointerAction::RelativeMove, 4, 5, 6)
            .simulate);
    }

    #[test]
    fn disconnect_releases_every_button_owned_by_connection() {
        let mut arbiter = PointerArbiter::default();
        arbiter.decide(7, PointerAction::Move, 1, 2, 1);
        arbiter.decide(7, PointerAction::Down(1), 0, 0, 2);
        arbiter.decide(7, PointerAction::Down(2), 0, 0, 3);
        assert_eq!(arbiter.clear_connection(7), vec![1, 2]);
        assert!(arbiter.decide(8, PointerAction::Down(1), 0, 0, 4).simulate);
    }

    #[test]
    fn stale_gesture_is_released_before_new_owner_starts() {
        let mut arbiter = PointerArbiter::default();
        arbiter.decide(1, PointerAction::Move, 10, 20, 1);
        arbiter.decide(1, PointerAction::Down(1), 0, 0, 2);
        arbiter.decide(2, PointerAction::Move, 30, 40, 3);
        let decision = arbiter.decide(2, PointerAction::Down(1), 0, 0, GESTURE_IDLE_TIMEOUT_MS + 3);
        assert_eq!(decision.release_stale_buttons, vec![1]);
        assert!(decision.simulate);
        assert_eq!(decision.move_before_action, Some((30, 40)));
    }

    #[test]
    fn collaborative_relative_mode_never_moves_system_cursor() {
        let mut arbiter = PointerArbiter::default();
        assert!(
            !arbiter
                .decide(1, PointerAction::RelativeMove, 50, -10, 1)
                .simulate
        );
    }
}
