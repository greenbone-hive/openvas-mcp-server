//! Tests for toolset selection and read-only gating: what the server
//! actually exposes, per configuration.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use support::config_with_args;
use wiremock::MockServer;

#[tokio::test]
async fn default_exposes_system_targets_and_tasks_tools() {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(&server, &[])).unwrap();
    let names = mcp.tool_names();

    for expected in [
        "openvas_test_connection",
        "openvas_get_version",
        "openvas_list_targets",
        "openvas_get_target",
        "openvas_list_tasks",
        "openvas_get_task",
        "openvas_create_task",
        "openvas_start_task",
        "openvas_stop_task",
        "openvas_resume_task",
        "openvas_delete_task",
    ] {
        assert!(names.contains(&expected.to_string()), "missing {expected}");
    }
    // The gateway removed its ticket routes; no tool may still point at them.
    assert!(
        !names.iter().any(|name| name.contains("ticket")),
        "ticket tools must be gone, got: {names:?}"
    );
}

#[tokio::test]
async fn read_only_hides_every_mutating_tool() {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(&server, &["--read-only"])).unwrap();
    let names = mcp.tool_names();

    for read_tool in [
        "openvas_test_connection",
        "openvas_list_targets",
        "openvas_list_tasks",
        "openvas_get_task",
    ] {
        assert!(
            names.contains(&read_tool.to_string()),
            "missing {read_tool}"
        );
    }
    for write_tool in [
        "openvas_create_task",
        "openvas_start_task",
        "openvas_stop_task",
        "openvas_resume_task",
        "openvas_delete_task",
    ] {
        assert!(
            !names.contains(&write_tool.to_string()),
            "{write_tool} must be hidden in read-only mode"
        );
    }
}

#[tokio::test]
async fn toolset_selection_limits_exposure() {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(&server, &["--toolsets", "targets"])).unwrap();
    let names = mcp.tool_names();

    assert!(names.contains(&"openvas_list_targets".to_string()));
    // System rides along for openvas_test_connection.
    assert!(names.contains(&"openvas_test_connection".to_string()));
    assert!(
        !names.iter().any(|name| name.contains("task")),
        "task tools must be absent, got: {names:?}"
    );
}

#[tokio::test]
async fn default_tool_count_matches_wired_toolsets() {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(&server, &[])).unwrap();
    // 89 read tools + 82 writes across every non-identity gateway family
    // (see docs/gateway-sync.md for the endpoint → tool matrix). A mismatch
    // means a router was not wired into server.rs (or a tool was added
    // without updating this inventory).
    assert_eq!(mcp.tool_names().len(), 171, "got: {:?}", mcp.tool_names());

    let read_only = GvmMcpServer::new(config_with_args(&server, &["--read-only"])).unwrap();
    // Every mutating tool disappears in read-only mode (report drill-down,
    // export and download stay: they mutate nothing durable).
    assert_eq!(read_only.tool_names().len(), 89);
    assert!(
        !read_only
            .tool_names()
            .iter()
            .any(|name| name.contains("create")
                || name.contains("update")
                || name.contains("delete")
                || name.contains("clone")
                || name.starts_with("openvas_start_")
                || name.starts_with("openvas_stop_")
                || name.starts_with("openvas_resume_")
                || name.starts_with("openvas_set_")
                || name.contains("import")
                || name.contains("verify")
                || name.contains("sync")),
        "read-only must hide all mutating tools"
    );
}

#[tokio::test]
async fn identity_toolset_is_opt_in() {
    let server = MockServer::start().await;

    let default = GvmMcpServer::new(config_with_args(&server, &[])).unwrap();
    assert!(
        !default
            .tool_names()
            .iter()
            .any(|name| name.contains("user")),
        "identity tools must be absent by default"
    );

    let with_identity = GvmMcpServer::new(config_with_args(
        &server,
        &["--toolsets", "default,identity"],
    ))
    .unwrap();
    let names = with_identity.tool_names();
    // 171 default + 23 identity (5 users + 5 groups + 5 roles +
    // 5 permissions + 3 user-settings).
    assert_eq!(names.len(), 194, "got: {names:?}");
    for expected in [
        "openvas_list_users",
        "openvas_create_user",
        "openvas_list_groups",
        "openvas_delete_role",
        "openvas_create_permission",
        "openvas_update_user_setting",
    ] {
        assert!(names.contains(&expected.to_string()), "missing {expected}");
    }

    // Identity + read-only: default reads (89) plus the 10 identity reads.
    let read_only = GvmMcpServer::new(config_with_args(
        &server,
        &["--toolsets", "default,identity", "--read-only"],
    ))
    .unwrap();
    assert_eq!(read_only.tool_names().len(), 99);
    assert!(
        read_only
            .tool_names()
            .contains(&"openvas_list_users".to_string())
    );
    assert!(
        !read_only
            .tool_names()
            .contains(&"openvas_create_user".to_string())
    );
}

#[tokio::test]
async fn read_only_and_selection_compose() {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(
        &server,
        &["--toolsets", "tasks", "--read-only"],
    ))
    .unwrap();
    let names = mcp.tool_names();

    assert!(names.contains(&"openvas_list_tasks".to_string()));
    assert!(names.contains(&"openvas_get_task".to_string()));
    assert!(!names.contains(&"openvas_start_task".to_string()));
    assert!(!names.contains(&"openvas_list_targets".to_string()));
}
