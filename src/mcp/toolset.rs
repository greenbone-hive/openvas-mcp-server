//! Toolset grouping and gating.
//!
//! The full gateway surface is ~190 tools — far too many to expose to an LLM
//! by default. Tools are grouped into toolsets selectable via `--toolsets`;
//! the identity toolset is always opt-in, and `--read-only` additionally
//! gates out every mutating tool.

use std::collections::BTreeSet;

use anyhow::bail;

/// One toolset ≈ one gateway resource family (roadmap §4). The set mirrors
/// the tag families of the rust-gvm-api OpenAPI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Toolset {
    System,
    Targets,
    Tasks,
    ScanConfigs,
    Scanners,
    Schedules,
    Credentials,
    Alerts,
    PortLists,
    Results,
    Reports,
    Assets,
    TlsCertificates,
    ReportFormats,
    Filters,
    Tags,
    Notes,
    Overrides,
    Nvts,
    SecInfo,
    Vulnerabilities,
    Feeds,
    Compliance,
    Agents,
    Identity,
}

impl Toolset {
    pub const ALL: &[Toolset] = &[
        Toolset::System,
        Toolset::Targets,
        Toolset::Tasks,
        Toolset::ScanConfigs,
        Toolset::Scanners,
        Toolset::Schedules,
        Toolset::Credentials,
        Toolset::Alerts,
        Toolset::PortLists,
        Toolset::Results,
        Toolset::Reports,
        Toolset::Assets,
        Toolset::TlsCertificates,
        Toolset::ReportFormats,
        Toolset::Filters,
        Toolset::Tags,
        Toolset::Notes,
        Toolset::Overrides,
        Toolset::Nvts,
        Toolset::SecInfo,
        Toolset::Vulnerabilities,
        Toolset::Feeds,
        Toolset::Compliance,
        Toolset::Agents,
        Toolset::Identity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Toolset::System => "system",
            Toolset::Targets => "targets",
            Toolset::Tasks => "tasks",
            Toolset::ScanConfigs => "scan-configs",
            Toolset::Scanners => "scanners",
            Toolset::Schedules => "schedules",
            Toolset::Credentials => "credentials",
            Toolset::Alerts => "alerts",
            Toolset::PortLists => "port-lists",
            Toolset::Results => "results",
            Toolset::Reports => "reports",
            Toolset::Assets => "assets",
            Toolset::TlsCertificates => "tls-certificates",
            Toolset::ReportFormats => "report-formats",
            Toolset::Filters => "filters",
            Toolset::Tags => "tags",
            Toolset::Notes => "notes",
            Toolset::Overrides => "overrides",
            Toolset::Nvts => "nvts",
            Toolset::SecInfo => "secinfo",
            Toolset::Vulnerabilities => "vulnerabilities",
            Toolset::Feeds => "feeds",
            Toolset::Compliance => "compliance",
            Toolset::Agents => "agents",
            Toolset::Identity => "identity",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Toolset::System => "Connectivity, health, version and timezone information",
            Toolset::Targets => {
                "Scan targets (hosts, port lists), OCI image and web application targets"
            }
            Toolset::Tasks => "Scan tasks and their lifecycle (start/stop/resume/clone)",
            Toolset::ScanConfigs => {
                "Scan configurations, NVT/family selection, preferences and generic configs"
            }
            Toolset::Scanners => "Scanner instances",
            Toolset::Schedules => "Scan schedules",
            Toolset::Credentials => "Authenticated-scan credentials and credential stores",
            Toolset::Alerts => "Alerts and notifications",
            Toolset::PortLists => "Port lists",
            Toolset::Results => "Individual scan results",
            Toolset::Reports => "Scan reports, drill-down pages, imports and exports",
            Toolset::Assets => "Host, operating-system and generic assets",
            Toolset::TlsCertificates => "TLS certificates discovered by scans",
            Toolset::ReportFormats => "Report formats",
            Toolset::Filters => "Saved filters",
            Toolset::Tags => "Tags on resources",
            Toolset::Notes => "Notes on results",
            Toolset::Overrides => "Severity overrides",
            Toolset::Nvts => "NVTs and NVT families",
            Toolset::SecInfo => "CVE, CPE, CERT-Bund and DFN-CERT catalogs",
            Toolset::Vulnerabilities => "Vulnerability listing",
            Toolset::Feeds => "Feed status",
            Toolset::Compliance => "Compliance audits and policies",
            Toolset::Agents => "Sensor agents and agent groups",
            Toolset::Identity => "Users, groups, roles, permissions (opt-in)",
        }
    }

    fn from_name(name: &str) -> Option<Toolset> {
        Self::ALL.iter().copied().find(|ts| ts.name() == name)
    }

    /// Toolsets exposed when no explicit selection is given: everything
    /// except identity (roadmap: identity off by default).
    fn default_enabled() -> impl Iterator<Item = Toolset> {
        Self::ALL
            .iter()
            .copied()
            .filter(|ts| *ts != Toolset::Identity)
    }
}

/// The set of toolsets this server instance exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsetSelection {
    enabled: BTreeSet<Toolset>,
}

impl ToolsetSelection {
    /// Parse `--toolsets` values. Accepts toolset names plus the specials
    /// `default` (all minus identity) and `all` (still minus identity —
    /// identity must be named explicitly). Empty input means `default`.
    pub fn parse(names: &[String]) -> anyhow::Result<Self> {
        let mut enabled = BTreeSet::new();
        if names.is_empty() {
            enabled.extend(Toolset::default_enabled());
            return Ok(Self { enabled });
        }

        for raw in names {
            let name = raw.trim().to_ascii_lowercase();
            match name.as_str() {
                "" => continue,
                "default" | "all" => enabled.extend(Toolset::default_enabled()),
                _ => match Toolset::from_name(&name) {
                    Some(ts) => {
                        enabled.insert(ts);
                    }
                    None => bail!(
                        "unknown toolset '{raw}'; run with --list-toolsets to see valid names"
                    ),
                },
            }
        }

        if enabled.is_empty() {
            bail!("toolset selection is empty");
        }
        // The system toolset carries openvas_test_connection; a server
        // without it cannot even be smoke-tested, so it is always on.
        enabled.insert(Toolset::System);
        Ok(Self { enabled })
    }

    pub fn is_enabled(&self, toolset: Toolset) -> bool {
        self.enabled.contains(&toolset)
    }

    pub fn iter(&self) -> impl Iterator<Item = Toolset> + '_ {
        self.enabled.iter().copied()
    }
}

impl std::fmt::Display for ToolsetSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.enabled.iter().map(|ts| ts.name()).collect();
        write!(f, "{}", names.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_selection_excludes_identity() {
        let selection = ToolsetSelection::parse(&[]).unwrap();
        assert!(selection.is_enabled(Toolset::Targets));
        assert!(selection.is_enabled(Toolset::System));
        assert!(selection.is_enabled(Toolset::Compliance));
        assert!(selection.is_enabled(Toolset::Agents));
        assert!(!selection.is_enabled(Toolset::Identity));
    }

    #[test]
    fn identity_must_be_named_explicitly() {
        let all = ToolsetSelection::parse(&["all".into()]).unwrap();
        assert!(!all.is_enabled(Toolset::Identity));

        let with_identity =
            ToolsetSelection::parse(&["default".into(), "identity".into()]).unwrap();
        assert!(with_identity.is_enabled(Toolset::Identity));
    }

    #[test]
    fn explicit_selection_is_exact_plus_system() {
        let selection = ToolsetSelection::parse(&["tasks".into(), "targets".into()]).unwrap();
        assert!(selection.is_enabled(Toolset::Tasks));
        assert!(selection.is_enabled(Toolset::Targets));
        assert!(selection.is_enabled(Toolset::System));
        assert!(!selection.is_enabled(Toolset::Reports));
    }

    #[test]
    fn names_are_case_insensitive_and_trimmed() {
        let selection = ToolsetSelection::parse(&[" Tasks ".into()]).unwrap();
        assert!(selection.is_enabled(Toolset::Tasks));
    }

    #[test]
    fn unknown_toolset_is_rejected() {
        let err = ToolsetSelection::parse(&["bogus".into()]).unwrap_err();
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn removed_ticket_toolset_is_unknown() {
        // The gateway removed its ticket routes (rust-gvm-api #500/#504);
        // naming the old toolset must fail loudly instead of silently
        // exposing tools that 404.
        let err = ToolsetSelection::parse(&["tickets".into()]).unwrap_err();
        assert!(err.to_string().contains("tickets"));
    }

    #[test]
    fn every_toolset_has_unique_name() {
        let names: BTreeSet<&str> = Toolset::ALL.iter().map(|ts| ts.name()).collect();
        assert_eq!(names.len(), Toolset::ALL.len());
    }
}
