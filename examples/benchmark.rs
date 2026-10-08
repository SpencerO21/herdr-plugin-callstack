use herdr_callstack::{model::Scope, store::Store, ui::View};
use std::{path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 4,
        "Usage: benchmark STATE_DIR NAMESPACE WORKSPACE SESSION"
    );
    let scope = Scope {
        namespace: args[1].clone(),
        workspace: args[2].clone(),
        session: args[3].clone(),
    };
    let store = Store::new(Path::new(&args[0]), scope.clone())?;
    let records = store.list()?;
    let flows = records.len();
    let mut view = View::new(scope, None);
    view.update(records);
    for _ in 0..100 {
        std::hint::black_box(view.render(140, 60));
    }
    let mut samples = Vec::new();
    for _ in 0..1000 {
        let start = Instant::now();
        std::hint::black_box(view.render(140, 60));
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "flows={flows} iterations=1000 cached_render_median_ms={:.6} p95_ms={:.6}",
        samples[500], samples[950]
    );
    Ok(())
}
