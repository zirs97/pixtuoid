use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ascii_agents_core::source::{Activity, AgentEvent};
use ascii_agents_core::state::reducer::{Reducer, Transport};
use ascii_agents_core::state::{ActivityState, SceneState};
use ascii_agents_core::AgentId;

fn start(reducer: &mut Reducer, scene: &mut SceneState, id: AgentId) {
    reducer.apply(
        scene,
        AgentEvent::SessionStart {
            agent_id: id,
            source: "claude-code".into(),
            session_id: "abc".into(),
            cwd: PathBuf::from("/"),
        },
        SystemTime::now(),
        Transport::Hook,
    );
}

#[test]
fn session_start_creates_idle_slot_at_first_free_desk() {
    let mut scene = SceneState::new(4);
    let mut reducer = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");

    reducer.apply(
        &mut scene,
        AgentEvent::SessionStart {
            agent_id: id,
            source: "claude-code".into(),
            session_id: "abc".into(),
            cwd: PathBuf::from("/repo"),
        },
        SystemTime::now(),
        Transport::Hook,
    );

    let slot = scene.agents.get(&id).expect("agent inserted");
    assert_eq!(slot.desk_index, 0);
    assert_eq!(&*slot.label, "repo", "label derived from cwd basename");
    assert_eq!(slot.state, ActivityState::Idle);
}

#[test]
fn activity_start_sets_state_active() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);

    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Typing,
            tool_use_id: Some("t1".into()),
            detail: Some("Edit: foo.rs".into()),
        },
        SystemTime::now(),
        Transport::Hook,
    );

    let slot = scene.agents.get(&id).unwrap();
    assert!(matches!(
        slot.state,
        ActivityState::Active {
            activity: Activity::Typing,
            ..
        }
    ));
}

#[test]
fn activity_end_returns_to_idle() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Typing,
            tool_use_id: Some("t1".into()),
            detail: None,
        },
        SystemTime::now(),
        Transport::Hook,
    );
    r.apply(
        &mut scene,
        AgentEvent::ActivityEnd {
            agent_id: id,
            tool_use_id: Some("t1".into()),
        },
        SystemTime::now(),
        Transport::Hook,
    );

    assert_eq!(scene.agents.get(&id).unwrap().state, ActivityState::Idle);
}

#[test]
fn waiting_sets_state_with_reason() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);

    r.apply(
        &mut scene,
        AgentEvent::Waiting {
            agent_id: id,
            reason: "Bash: rm -rf?".into(),
        },
        SystemTime::now(),
        Transport::Hook,
    );

    match &scene.agents.get(&id).unwrap().state {
        ActivityState::Waiting { reason } => assert_eq!(&**reason, "Bash: rm -rf?"),
        other => panic!("unexpected state: {other:?}"),
    }
}

#[test]
fn session_end_marks_slot_exiting_then_tick_removes_it_after_grace() {
    use ascii_agents_core::state::reducer::EXIT_GRACE_WINDOW;
    let mut scene = SceneState::new(2);
    let mut r = Reducer::new();
    let a = AgentId::from_transcript_path("/p/a.jsonl");
    let b = AgentId::from_transcript_path("/p/b.jsonl");
    start(&mut r, &mut scene, a);
    start(&mut r, &mut scene, b);

    let t0 = SystemTime::now();
    r.apply(
        &mut scene,
        AgentEvent::SessionEnd { agent_id: a },
        t0,
        Transport::Hook,
    );

    let slot = scene
        .agents
        .get(&a)
        .expect("slot still present during exit walk-out");
    assert!(
        slot.exiting_at.is_some(),
        "SessionEnd should mark exiting_at"
    );

    r.tick(
        &mut scene,
        t0 + EXIT_GRACE_WINDOW + std::time::Duration::from_millis(100),
    );
    assert!(
        !scene.agents.contains_key(&a),
        "tick should sweep expired exit"
    );
    assert_eq!(scene.next_free_desk(), Some(0));
}

#[test]
fn jsonl_duplicate_of_recent_hook_is_dropped() {
    let mut scene = SceneState::new(2);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);

    let t0 = SystemTime::now();
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Typing,
            tool_use_id: Some("t-1".into()),
            detail: None,
        },
        t0,
        Transport::Hook,
    );

    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Reading,
            tool_use_id: Some("t-1".into()),
            detail: Some("FROM_JSONL".into()),
        },
        t0 + Duration::from_millis(100),
        Transport::Jsonl,
    );

    let slot = scene.agents.get(&id).unwrap();
    match &slot.state {
        ActivityState::Active {
            activity, detail, ..
        } => {
            assert_eq!(*activity, Activity::Typing, "hook event must win");
            assert_ne!(
                detail.as_deref(),
                Some("FROM_JSONL"),
                "jsonl detail must not overwrite"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

/// Bug 2: CC's hook payloads set transcript_path to the PARENT'S transcript
/// even for actions originating in a subagent. Those leak hook events onto
/// the parent's slot, making the parent sprite blink while the actual work
/// is in a subagent. Once the parent has a Task tool in flight, hook
/// ActivityStart/End events for that AgentId should be suppressed — the
/// JSONL stream is authoritative for the subagent (separate AgentId).
#[test]
fn hook_activity_during_active_task_is_suppressed() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let parent = AgentId::from_transcript_path("/p/parent.jsonl");
    start(&mut r, &mut scene, parent);

    let t0 = SystemTime::now();

    // Parent enters Task tool — hook fires first, carrying the tool_use_id.
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: parent,
            activity: Activity::Typing,
            tool_use_id: Some("task-T".into()),
            detail: Some("Task".into()),
        },
        t0,
        Transport::Hook,
    );

    // Subagent fires a Read hook. CC reports it on parent's transcript_path,
    // so it lands on parent's AgentId — we must drop it.
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: parent,
            activity: Activity::Typing,
            tool_use_id: Some("subagent-R".into()),
            detail: Some("Read: /foo".into()),
        },
        t0 + Duration::from_millis(50),
        Transport::Hook,
    );

    // Parent slot should still reflect Task (now rendered as "Delegating"
    // per ToolDetail::display), not the leaked Read.
    let slot = scene.agents.get(&parent).unwrap();
    match &slot.state {
        ActivityState::Active { detail, .. } => {
            assert_eq!(detail.as_deref(), Some("Delegating"));
        }
        other => panic!("expected Active(Delegating), got {other:?}"),
    }

    // Subagent's PostToolUse hook for Read also lands on parent — drop it.
    r.apply(
        &mut scene,
        AgentEvent::ActivityEnd {
            agent_id: parent,
            tool_use_id: Some("subagent-R".into()),
        },
        t0 + Duration::from_millis(60),
        Transport::Hook,
    );
    let slot = scene.agents.get(&parent).unwrap();
    assert!(
        matches!(slot.state, ActivityState::Active { .. }),
        "parent must remain Active(Task) while task in flight"
    );

    // Task's own PostToolUse: tool_use_id matches the in-flight Task, so the
    // hook IS allowed through, transitioning parent to Idle.
    r.apply(
        &mut scene,
        AgentEvent::ActivityEnd {
            agent_id: parent,
            tool_use_id: Some("task-T".into()),
        },
        t0 + Duration::from_millis(200),
        Transport::Hook,
    );
    assert_eq!(
        scene.agents.get(&parent).unwrap().state,
        ActivityState::Idle
    );
}

/// JSONL is the authoritative attribution for subagent work — its events
/// go to the subagent's own AgentId (different file path) and must NOT be
/// affected by the parent's active Task suppression.
#[test]
fn subagent_jsonl_activity_is_unaffected_by_parent_task_suppression() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let parent = AgentId::from_transcript_path("/p/parent.jsonl");
    let subagent = AgentId::from_transcript_path("/p/parent/subagents/agent-x.jsonl");
    start(&mut r, &mut scene, parent);
    start(&mut r, &mut scene, subagent);

    let t0 = SystemTime::now();
    // Parent enters a Task.
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: parent,
            activity: Activity::Typing,
            tool_use_id: Some("task-T".into()),
            detail: Some("Task".into()),
        },
        t0,
        Transport::Hook,
    );
    // Subagent's JSONL activity targets ITS OWN AgentId — must apply normally.
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: subagent,
            activity: Activity::Typing,
            tool_use_id: Some("sub-R".into()),
            detail: Some("Read: /bar".into()),
        },
        t0 + Duration::from_millis(120),
        Transport::Jsonl,
    );
    match &scene.agents.get(&subagent).unwrap().state {
        ActivityState::Active { detail, .. } => {
            assert_eq!(detail.as_deref(), Some("Read: /bar"));
        }
        other => panic!("subagent slot should be Active, got {other:?}"),
    }
}

/// Pre-existing behavior: with no active Task, hook events apply normally.
#[test]
fn hook_activity_without_active_task_applies_normally() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);

    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Typing,
            tool_use_id: Some("t".into()),
            detail: Some("Bash: ls".into()),
        },
        SystemTime::now(),
        Transport::Hook,
    );
    match &scene.agents.get(&id).unwrap().state {
        ActivityState::Active { detail, .. } => {
            assert_eq!(detail.as_deref(), Some("Bash: ls"));
        }
        other => panic!("expected Active, got {other:?}"),
    }
}

#[test]
fn session_start_with_cwd_derives_label_from_basename() {
    // No more "cc#1" when the cwd tells us what project this is.
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    r.apply(
        &mut scene,
        AgentEvent::SessionStart {
            agent_id: id,
            source: "claude-code".into(),
            session_id: "abc".into(),
            cwd: PathBuf::from("/Users/me/Desktop/ascii-agents"),
        },
        SystemTime::now(),
        Transport::Hook,
    );
    assert_eq!(&*scene.agents.get(&id).unwrap().label, "ascii-agents");
}

#[test]
fn session_start_without_cwd_falls_back_to_cc_label() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    r.apply(
        &mut scene,
        AgentEvent::SessionStart {
            agent_id: id,
            source: "claude-code".into(),
            session_id: "abc".into(),
            cwd: PathBuf::from(""),
        },
        SystemTime::now(),
        Transport::Hook,
    );
    assert_eq!(&*scene.agents.get(&id).unwrap().label, "cc#1");
}

#[test]
fn rename_updates_slot_label() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);
    r.apply(
        &mut scene,
        AgentEvent::Rename {
            agent_id: id,
            label: "feature-dev:code-explorer".into(),
        },
        SystemTime::now(),
        Transport::Jsonl,
    );
    assert_eq!(
        &*scene.agents.get(&id).unwrap().label,
        "feature-dev:code-explorer"
    );
}

#[test]
fn rename_for_unknown_agent_is_noop() {
    let mut scene = SceneState::new(4);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/missing.jsonl");
    r.apply(
        &mut scene,
        AgentEvent::Rename {
            agent_id: id,
            label: "x".into(),
        },
        SystemTime::now(),
        Transport::Jsonl,
    );
    assert!(scene.agents.get(&id).is_none());
}

#[test]
fn jsonl_event_after_dedup_window_is_applied() {
    let mut scene = SceneState::new(2);
    let mut r = Reducer::new();
    let id = AgentId::from_transcript_path("/p/a.jsonl");
    start(&mut r, &mut scene, id);

    let t0 = SystemTime::now();
    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Typing,
            tool_use_id: Some("t-1".into()),
            detail: None,
        },
        t0,
        Transport::Hook,
    );

    r.apply(
        &mut scene,
        AgentEvent::ActivityStart {
            agent_id: id,
            activity: Activity::Reading,
            tool_use_id: Some("t-1".into()),
            detail: None,
        },
        t0 + Duration::from_millis(600),
        Transport::Jsonl,
    );

    let slot = scene.agents.get(&id).unwrap();
    assert!(matches!(
        slot.state,
        ActivityState::Active {
            activity: Activity::Reading,
            ..
        }
    ));
}
