fn main() {
    if std::env::args().any(|s| s == "--versions") {
        println!(
            "{}",
            serde_json::to_string_pretty(&plantool_contract::Health::default()).unwrap()
        );
    } else {
        print!("{}", plantool_contract::schema());
    }
}
