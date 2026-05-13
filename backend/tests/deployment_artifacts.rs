use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[test]
fn shipped_quadlet_artifacts_define_oracy_scoped_ingress_fabric() {
    let quadlet_root = deploy_path("quadlet");
    let container = read_deploy("quadlet/oracy.container.in");
    let caddy_container = read_deploy("quadlet/oracy-caddy.container.in");
    let ingress_network = read_deploy("quadlet/oracy-ingress.network");
    let caddy_data = read_deploy("quadlet/oracy-caddy-data.volume");
    let caddy_config = read_deploy("quadlet/oracy-caddy-config.volume");

    assert_file_exists(quadlet_root.join("oracy.container.in"));
    assert_file_exists(quadlet_root.join("oracy-data.volume.in"));
    assert_file_exists(quadlet_root.join("oracy-ingress.network"));
    assert_file_exists(quadlet_root.join("oracy-caddy.container.in"));
    assert_file_exists(quadlet_root.join("oracy-caddy-data.volume"));
    assert_file_exists(quadlet_root.join("oracy-caddy-config.volume"));

    assert_contains_all(
        &container,
        &[
            "ContainerName=oracy",
            "Volume=@ORACY_CONFIG_PATH@:/etc/oracy/oracy.toml:ro,Z",
            "Volume=oracy-data.volume:/var/lib/oracy:rw,Z",
            "Network=oracy-ingress.network",
            "NetworkAlias=oracy",
            "PublishPort=127.0.0.1:@ORACY_OPERATOR_HOST_PORT@:9090",
            "StopTimeout=30",
            "TimeoutStopSec=40",
        ],
    );
    assert_contains_all(
        &read_deploy("quadlet/oracy-data.volume.in"),
        &["Device=@ORACY_STATE_DIR@", "Type=none", "Options=bind"],
    );
    assert_contains_all(
        &caddy_container,
        &[
            "ContainerName=oracy-caddy",
            "Network=oracy-ingress.network",
            "PublishPort=80:80",
            "PublishPort=443:443",
            "PublishPort=443:443/udp",
            "Volume=@ORACY_CADDYFILE_PATH@:/etc/caddy/Caddyfile:ro,Z",
            "Volume=oracy-caddy-data.volume:/data:rw,Z",
            "Volume=oracy-caddy-config.volume:/config:rw,Z",
        ],
    );
    assert_contains_all(
        &ingress_network,
        &["[Network]", "NetworkName=oracy-ingress"],
    );
    assert_contains_all(&caddy_data, &["[Volume]", "VolumeName=oracy-caddy-data"]);
    assert_contains_all(
        &caddy_config,
        &["[Volume]", "VolumeName=oracy-caddy-config"],
    );
}

#[test]
fn shipped_caddyfile_template_routes_oracy_public_api() {
    let caddyfile = read_deploy("examples/Caddyfile.in");

    assert_contains_all(
        &caddyfile,
        &[
            "@ORACY_PUBLIC_HOSTNAME@ {",
            "output stdout",
            "format json",
            "reverse_proxy http://oracy:8080",
            "header_up X-Real-IP {http.request.remote.host}",
            "read_timeout 300s",
            "write_timeout 300s",
            "X-Content-Type-Options nosniff",
            "X-Frame-Options DENY",
            "Referrer-Policy strict-origin-when-cross-origin",
            "-Server",
        ],
    );
}

#[test]
fn template_suffixes_match_placeholder_presence() {
    for artifact in deploy_files("quadlet")
        .into_iter()
        .chain(deploy_files("examples"))
    {
        let contents = std::fs::read_to_string(&artifact)
            .unwrap_or_else(|_| panic!("read {}", artifact.display()));
        let has_placeholder = !template_variables(&contents).is_empty();
        let has_template_suffix = artifact
            .extension()
            .is_some_and(|extension| extension == "in");

        assert_eq!(
            has_placeholder,
            has_template_suffix,
            "{} should use .in exactly when it contains @VAR@ placeholders",
            artifact.display()
        );
    }
}

#[test]
fn deployment_readme_documents_all_shipped_template_variables() {
    let readme = read_deploy("README.md");
    let variables = deploy_files("quadlet")
        .into_iter()
        .chain(deploy_files("examples"))
        .flat_map(|path| {
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("read {}", path.display()));
            template_variables(&contents)
        })
        .collect::<BTreeSet<_>>();

    for variable in variables {
        assert!(
            readme.contains(&format!("- `@{variable}@`:")),
            "README should document @{variable}@ in Operator-Owned Values"
        );
    }
}

#[test]
fn deployment_readme_install_section_names_every_shipped_quadlet() {
    let readme = read_deploy("README.md");
    let install = markdown_section(&readme, "Install Quadlets");

    for artifact in deploy_files("quadlet") {
        let filename = artifact
            .file_name()
            .and_then(|name| name.to_str())
            .expect("quadlet artifact should have utf-8 filename");

        assert!(
            install.contains(filename),
            "Install Quadlets section should name {filename}"
        );
    }
}

#[test]
fn deployment_readme_presents_the_caddy_shared_network_shape_as_canonical() {
    let readme = read_deploy("README.md");
    let install = markdown_section(&readme, "Install Quadlets");
    let deployment_shape = canonical_shape_tokens(&[
        read_deploy("quadlet/oracy.container.in"),
        read_deploy("quadlet/oracy-caddy.container.in"),
        read_deploy("quadlet/oracy-ingress.network"),
        read_deploy("examples/Caddyfile.in"),
    ]);

    for token in deployment_shape {
        assert!(
            install.contains(&token),
            "Install Quadlets section should expose canonical deployment token {token}"
        );
    }
}

#[test]
fn deployment_contract_names_the_supported_oracy_scoped_ingress_substrate() {
    let contract =
        std::fs::read_to_string("../spec/deployment.md").expect("read deployment contract");
    let public_api = markdown_section(&contract, "Public API Reverse Proxy");

    for token in [
        "oracy-ingress.network",
        "oracy-caddy.container",
        "oracy-caddy-data.volume",
        "oracy-caddy-config.volume",
        "Caddy",
        "http://oracy:8080",
    ] {
        assert!(
            public_api.contains(token),
            "deployment contract should name supported substrate token {token}"
        );
    }
}

fn deploy_path(relative: impl AsRef<Path>) -> PathBuf {
    Path::new("../deploy").join(relative)
}

fn read_deploy(relative: &str) -> String {
    let path = deploy_path(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {}", path.display()))
}

fn assert_file_exists(path: PathBuf) {
    assert!(path.is_file(), "{} should exist", path.display());
}

fn assert_contains_all(document: &str, required: &[&str]) {
    for item in required {
        assert!(document.contains(item), "document should contain {item}");
    }
}

fn deploy_files(relative: &str) -> Vec<PathBuf> {
    let root = deploy_path(relative);
    let mut files = std::fs::read_dir(&root)
        .unwrap_or_else(|_| panic!("read {}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|_| panic!("read directory entry from {}", root.display()))
                .path()
        })
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();

    files.sort();
    files
}

fn template_variables(contents: &str) -> BTreeSet<String> {
    let mut variables = BTreeSet::new();
    let bytes = contents.as_bytes();
    let mut index = 0;

    while let Some(start_offset) = contents[index..].find('@') {
        let start = index + start_offset;
        let Some(end_offset) = contents[start + 1..].find('@') else {
            break;
        };
        let end = start + 1 + end_offset;
        let candidate = &contents[start + 1..end];

        if !candidate.is_empty()
            && candidate
                .bytes()
                .all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && bytes.get(start) == Some(&b'@')
            && bytes.get(end) == Some(&b'@')
        {
            variables.insert(candidate.to_owned());
        }

        index = end + 1;
    }

    variables
}

fn canonical_shape_tokens(artifacts: &[String]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();

    for artifact in artifacts {
        for line in artifact.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "ContainerName" | "Network" | "NetworkAlias" | "NetworkName" | "VolumeName"
                    if !value.starts_with('@') =>
                {
                    tokens.insert(value.to_owned());
                }
                "Volume" => {
                    if let Some((source, _)) = value.split_once(':') {
                        if !source.starts_with('@') {
                            tokens.insert(source.to_owned());
                        }
                    }
                }
                "reverse_proxy" => {
                    tokens.insert(value.trim().to_owned());
                }
                _ => {}
            }
        }
    }

    tokens
}

fn markdown_section<'a>(document: &'a str, heading: &str) -> &'a str {
    let start_marker = format!("## {heading}");
    let start = document
        .find(&start_marker)
        .unwrap_or_else(|| panic!("{heading} section should exist"));
    let after_heading = start + start_marker.len();
    let end = document[after_heading..]
        .find("\n## ")
        .map_or(document.len(), |offset| after_heading + offset);

    &document[start..end]
}
