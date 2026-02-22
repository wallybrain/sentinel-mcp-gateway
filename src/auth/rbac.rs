use crate::config::types::RbacConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Read,
    Execute,
}

impl Permission {
    pub fn required_permission_str(&self) -> &str {
        match self {
            Permission::Read => "tools.read",
            Permission::Execute => "tools.execute",
        }
    }
}

pub fn is_tool_allowed(
    role: &str,
    tool_name: &str,
    permission: Permission,
    rbac: &RbacConfig,
) -> bool {
    let Some(role_config) = rbac.roles.get(role) else {
        return false;
    };

    if role_config.denied_tools.iter().any(|d| d == tool_name) {
        return false;
    }

    if role_config.permissions.iter().any(|p| p == "*") {
        return true;
    }

    let required = permission.required_permission_str();
    if role_config.permissions.iter().any(|p| p == required) {
        return true;
    }

    // tools.execute implies tools.read
    if permission == Permission::Read
        && role_config
            .permissions
            .iter()
            .any(|p| p == "tools.execute")
    {
        return true;
    }

    false
}
