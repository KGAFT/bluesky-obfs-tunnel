use super::*;

fn parse(args: &[&str]) -> io::Result<Option<Options>> {
    Options::parse(args.iter().map(|arg| arg.to_string()))
}

#[test]
fn listener_and_fixed_destination_cli_overrides() {
    let options = parse(&["--bind", "127.0.0.1:9443", "--tcp", "127.0.0.1:2081"])
        .unwrap()
        .unwrap();
    let mut settings = ServerSettings {
        forwarding: ForwardMode::Multiplexed,
        ..ServerSettings::default()
    };
    options.apply(&mut settings).unwrap();
    assert_eq!(settings.bind_address, "127.0.0.1:9443");
    assert_eq!(
        settings.forwarding,
        ForwardMode::Tcp {
            destination: "127.0.0.1:2081".parse().unwrap(),
        }
    );
    let options = parse(&["--bind", "[::1]:9443", "--multiplexed"])
        .unwrap()
        .unwrap();
    options.apply(&mut settings).unwrap();
    assert_eq!(settings.bind_address, "[::1]:9443");
    assert_eq!(settings.forwarding, ForwardMode::Multiplexed);
    for args in [
        vec!["--bind"],
        vec!["--tcp"],
        vec!["--bind", "localhost:9080"],
        vec!["--tcp", "127.0.0.1:0"],
        vec!["--tcp", "127.0.0.1:2080", "--multiplexed"],
        vec!["--bind", "127.0.0.1:9080", "--bind", "127.0.0.1:9081"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}

#[test]
fn config_arguments_and_forwarding_override() {
    for args in [
        vec!["--config", "custom.json", "--multiplexed"],
        vec!["--multiplexed", "--config=custom.json"],
    ] {
        let options = parse(&args).unwrap().unwrap();
        assert_eq!(options.config, "custom.json");
        assert!(options.multiplexed);
    }
    assert!(parse(&["--help"]).unwrap().is_none());
    for args in [
        vec!["--config"],
        vec!["--config="],
        vec!["--config", "--multiplexed"],
        vec!["--unknown"],
    ] {
        assert!(parse(&args).is_err());
    }
}

#[test]
fn client_creation_is_a_separate_command() {
    let options = parse(&["--config", "custom.json", "--create-client", "alice"])
        .unwrap()
        .unwrap();
    assert_eq!(options.create_client.as_deref(), Some("alice"));
    for args in [
        vec!["--create-client"],
        vec!["--create-client", ""],
        vec!["--create-client", "--multiplexed"],
        vec!["--create-client", "alice", "--multiplexed"],
        vec!["--create-client", "alice", "--bind", "127.0.0.1:9080"],
        vec!["--create-client", "alice", "--create-client", "bob"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}
