fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Pass the signed compatibility envelope.")?;
    let revision = spice_route_core::rule_updates::verified_revision(&std::fs::read(path)?)?;
    println!("{revision}");
    Ok(())
}
