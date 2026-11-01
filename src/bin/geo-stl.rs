//! CLI STL utilisant le même cache et les mêmes workers que le serveur.

use clap::{CommandFactory, FromArgMatches};
use geo_platform::{
    cli::{ExportCli, run_export},
    domain::ExportFormat,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(arguments) = geo_platform::backend::engine_worker_arguments() {
        return geo_platform::backend::run_engine_worker(arguments).await;
    }
    let mut command = ExportCli::command();
    command = command
        .name("geo-stl")
        .about("Crée un export STL à partir des données géographiques en cache");
    let matches = command.get_matches();
    let mut arguments = ExportCli::from_arg_matches(&matches)?;
    if arguments
        .format
        .is_some_and(|format| format != ExportFormat::Stl)
    {
        anyhow::bail!("geo-stl ne produit que le format stl");
    }
    arguments.format = Some(ExportFormat::Stl);
    let job = run_export(arguments).await?;
    println!("{}", serde_json::to_string_pretty(&job)?);
    Ok(())
}
