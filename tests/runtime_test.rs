use agentic_armor::RuntimeChoice;

#[test]
fn test_runtime_choice_default() {
    assert_eq!(RuntimeChoice::default(), RuntimeChoice::Auto);
}

#[test]
fn test_runtime_choice_from_docker() {
    assert_eq!(RuntimeChoice::parse("docker"), RuntimeChoice::Docker);
    assert_eq!(RuntimeChoice::parse("DOCKER"), RuntimeChoice::Docker);
    assert_eq!(RuntimeChoice::parse("Docker"), RuntimeChoice::Docker);
}

#[test]
fn test_runtime_choice_from_podman() {
    assert_eq!(RuntimeChoice::parse("podman"), RuntimeChoice::Podman);
    assert_eq!(RuntimeChoice::parse("PODMAN"), RuntimeChoice::Podman);
}

#[test]
fn test_runtime_choice_from_auto() {
    assert_eq!(RuntimeChoice::parse("auto"), RuntimeChoice::Auto);
}

#[test]
fn test_runtime_choice_from_unknown_defaults_to_auto() {
    assert_eq!(RuntimeChoice::parse(""), RuntimeChoice::Auto);
    assert_eq!(RuntimeChoice::parse("invalid"), RuntimeChoice::Auto);
    assert_eq!(RuntimeChoice::parse("containerd"), RuntimeChoice::Auto);
}
