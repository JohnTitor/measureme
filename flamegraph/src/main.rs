use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use measureme::ProfilingData;

use structopt::StructOpt;

use tools_lib::stack_collapse::collapse_stacks;

use inferno::flamegraph::{from_lines, Options as FlamegraphOptions};

#[derive(StructOpt, Debug)]
struct Opt {
    file_prefix: PathBuf,

    /// The sampling interval in milliseconds
    #[structopt(short = "i", long = "interval", default_value = "1")]
    interval: u64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();

    let profiling_data = ProfilingData::new(&opt.file_prefix)?;

    let recorded_stacks = collapse_stacks(profiling_data.iter(), opt.interval)
        .iter()
        .map(|(unique_stack, count)| format!("{} {}", unique_stack, count))
        .collect::<Vec<_>>();

    let file = BufWriter::new(File::create("rustc.svg")?);
    let mut flamegraph_options = FlamegraphOptions::default();

    from_lines(
        &mut flamegraph_options,
        recorded_stacks.iter().map(|s| s.as_ref()),
        file,
    )
    .expect(
        "unable to generate a flamegraph \
         from the collapsed stack data",
    );

    Ok(())
}
