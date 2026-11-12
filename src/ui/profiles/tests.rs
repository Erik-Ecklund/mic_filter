use super::*;
use crate::config::AppConfig;
use crate::ui::tests::app;

#[test]
fn management_actions_preserve_working_chain_and_saved_snapshots() {
    let mut app = app(AppConfig::default());
    let state = &mut app.state;
    state.profile_ui.name = "Meetings".into();
    assert!(state.profile_action(Action::SaveNew).unwrap());
    let saved = state.config.profiles[0].clone();
    state.config.filters.clear();
    state
        .params
        .publish_chain(&state.config.filters, &state.config.limiter);
    state.profile_ui.name = "Gaming".into();
    state.profile_action(Action::Duplicate(0)).unwrap();
    assert_eq!(state.config.profiles[1].filters, saved.filters);
    assert!(state.config.filters.is_empty());
    state.profile_ui.name = "Streaming".into();
    state.profile_action(Action::Rename(1)).unwrap();
    assert_eq!(state.profile_ui.selected.as_deref(), Some("Streaming"));
    state.profile_action(Action::Overwrite(1)).unwrap();
    assert!(state.config.profiles[1].filters.is_empty());
    assert_eq!(state.config.profiles[0], saved);
    state.profile_action(Action::Export(0)).unwrap();
    let exported = state.profile_ui.exchange.clone();
    assert!(state.profile_action(Action::Import).is_err());
    state.profile_ui.exchange = exported.replace("Meetings", "Imported");
    state.profile_action(Action::Import).unwrap();
    assert_eq!(state.profile_ui.selected.as_deref(), Some("Imported"));
    assert!(state.config.filters.is_empty());
    state.profile_action(Action::Load(2)).unwrap();
    assert!(saved.matches(&state.config));
    state.profile_action(Action::Delete(2)).unwrap();
    assert!(state.profile_ui.selected.is_none());
    assert!(saved.matches(&state.config));
}
