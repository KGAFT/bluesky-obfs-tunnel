use super::*;

#[test]
fn combined_addresses_support_both_protocols_hostnames_and_bracketed_ipv6() {
    for (flag, protocol) in [
        ("--tcp", ForwardProtocol::Tcp),
        ("--udp", ForwardProtocol::Udp),
    ] {
        for (value, destination, local) in [
            (
                "127.0.0.1:8090:127.0.0.1:1080",
                "127.0.0.1:8090",
                "127.0.0.1:1080",
            ),
            (
                "localhost:2080:0.0.0.0:1080",
                "localhost:2080",
                "0.0.0.0:1080",
            ),
            ("[::1]:8090:[::1]:1080", "[::1]:8090", "[::1]:1080"),
            (
                "[2001:db8::1]:8090:127.0.0.1:0",
                "[2001:db8::1]:8090",
                "127.0.0.1:0",
            ),
            ("example.org:53:[::]:1053", "example.org:53", "[::]:1053"),
        ] {
            let options =
                Options::parse([flag, value, "--server", "tunnel:9443"].map(String::from))
                    .unwrap()
                    .unwrap();
            assert_eq!(
                options.destination,
                Some((protocol, destination.to_owned()))
            );
            assert_eq!(options.listen, Some(local.parse().unwrap()));
            assert!(!options.is_proxy());
            let mut settings = ClientSettings::default();
            options.apply(&mut settings).unwrap();
            assert_eq!(settings.server_address, "tunnel:9443");
        }
        for destination in ["127.0.0.1:8090", "localhost:53", "[::1]:53"] {
            let options = Options::parse([flag, destination].map(String::from))
                .unwrap()
                .unwrap();
            assert_eq!(
                options.destination,
                Some((protocol, destination.to_owned()))
            );
            assert!(options.listen.is_none());
        }
    }
}

#[test]
fn rejects_malformed_addresses_and_conflicting_forwarding_modes() {
    for flag in ["--tcp", "--udp"] {
        for value in [
            "",
            "localhost",
            ":80",
            "localhost:0",
            "localhost:65536",
            "localhost:x",
            "127.0.0.1:80:",
            "127.0.0.1:80:localhost:1080",
            "127.0.0.1:80:127.0.0.1",
            "127.0.0.1:80:127.0.0.1:65536",
            "127.0.0.1:80:127.0.0.1:1080:extra",
            "::1:80",
            "[::1:80",
            "[::1]80",
            "[bad]:80",
            "http://localhost:80",
            "local host:80",
            "[::1]:80:::1:1080",
        ] {
            assert!(
                Options::parse([flag, value].map(String::from)).is_err(),
                "{flag} {value}"
            );
        }
        for conflict in [
            vec!["--message", "x"],
            vec!["--file", "payload.bin"],
            vec!["--proxy"],
            vec!["--http-proxy", "127.0.0.1:8080"],
            vec!["--tcp", "localhost:80"],
            vec!["--udp", "localhost:53"],
            vec!["--listen", "127.0.0.1:1080"],
        ] {
            let mut args = vec![flag, "localhost:8090:127.0.0.1:1080"];
            args.extend(conflict);
            assert!(
                Options::parse(args.iter().map(|s| s.to_string())).is_err(),
                "{args:?}"
            );
        }
    }
}
