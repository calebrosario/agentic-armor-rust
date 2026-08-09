use agentic_armor::RuntimeChoice;

#[test]
fn test_runtime_choice_default() {
    assert_eq!(RuntimeChoice::default(), RuntimeChoice::Auto);
}

#[test]
fn test_runtime_choice_from_docker() {
    assert_eq!(RuntimeChoice::from_str("docker"), RuntimeChoice::Docker);
    assert_eq!(RuntimeChoice::from_str("DOCKER"), RuntimeChoice::Docker);
    assert_eq!(RuntimeChoice::from_str("Docker"), RuntimeChoice::Docker);
}

#[test]
fn test_runtime_choice_from_podman() {
    assert_eq!(RuntimeChoice::from_str("podman"), RuntimeChoice::Podman);
    assert_eq!(RuntimeChoice::from_str("PODMAN"), RuntimeChoice::Podman);
}

#[test]
fn test_runtime_choice_from_auto() {
    assert_eq!(RuntimeChoice::from_str("auto"), RuntimeChoice::Auto);
}

#[test]
fn test_runtime_choice_from_unknown_defaults_to_auto() {
    assert_eq!(RuntimeChoice::from_str(""), RuntimeChoice::Auto);
    assert_eq!(RuntimeChoice::from_str("invalid"), RuntimeChoice::Auto);
    assert_eq!(RuntimeChoice::from_str("containerd"), RuntimeChoice::Auto);
}
