use agentic_armor::ArmorError;

#[test]
fn test_error_codes() {
    assert_eq!(
        ArmorError::TaskNotFound("x".into()).code(),
        "TASK_NOT_FOUND"
    );
    assert_eq!(
        ArmorError::ForbiddenMount("x".into()).code(),
        "FORBIDDEN_MOUNT"
    );
    assert_eq!(ArmorError::HostNetworkDenied.code(), "HOST_NETWORK_DENIED");
    assert_eq!(
        ArmorError::InvalidNetworkMode("x".into()).code(),
        "INVALID_NETWORK_MODE"
    );
    assert_eq!(
        ArmorError::InvalidMountConfig("x".into()).code(),
        "INVALID_MOUNT_CONFIG"
    );
    assert_eq!(ArmorError::InvalidPath("x".into()).code(), "INVALID_PATH");
    assert_eq!(ArmorError::PathRestricted.code(), "PATH_RESTRICTED");
    assert_eq!(
        ArmorError::ContainerCreateFailed("x".into()).code(),
        "CONTAINER_CREATE_FAILED"
    );
    assert_eq!(
        ArmorError::DockerConnectionFailed("x".into()).code(),
        "DOCKER_CONNECTION_FAILED"
    );
    assert_eq!(ArmorError::Docker("x".into()).code(), "DOCKER_ERROR");
    assert_eq!(ArmorError::Database("x".into()).code(), "DATABASE_ERROR");
}

#[test]
fn test_error_display() {
    let e = ArmorError::ForbiddenMount("docker.sock".into());
    assert!(e.to_string().contains("docker.sock"));

    let e = ArmorError::TaskNotFound("task-123".into());
    assert!(e.to_string().contains("task-123"));

    let e = ArmorError::HostNetworkDenied;
    assert!(e.to_string().contains("ALLOW_HOST_NETWORK"));
}
