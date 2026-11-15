diesel::table! {
    clients (identity) {
        identity -> Binary,
        password -> Binary,
        last_auth_ms -> Nullable<BigInt>,
    }
}
