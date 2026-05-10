use criterion::{Criterion, criterion_group, criterion_main};
use pxtone::{DestinationQuality, PxtoneService, VomitPreparation};

fn bench_moo(c: &mut Criterion) {
  let files = [
    (
      "overworld2_orche",
      "tests/sample/ptcop/overworld2_orche[Se-ko].ptcop",
    ),
    (
      "overworld2_nes",
      "tests/sample/ptcop/overworld2_nes[Se-ko].ptcop",
    ),
    ("5LOVE", "tests/sample/ptcop/5LOVE[Quois].ptcop"),
  ];

  for (name, path) in &files {
    let data = std::fs::read(path).unwrap();

    c.bench_function(name, |b| {
      b.iter_batched(
        || {
          let mut svc = PxtoneService::new(DestinationQuality::default());
          svc.read(data.clone()).unwrap();
          svc.tones_ready().unwrap();
          svc.moo_preparation(VomitPreparation::default()).unwrap();
          svc
        },
        |mut svc| {
          let q = svc.get_destination_quality();
          let mut buf = vec![0u8; q.channels as usize * 2 * 4096];
          loop {
            if svc.moo(&mut buf) == 0 {
              break;
            }
          }
        },
        criterion::BatchSize::LargeInput,
      )
    });
  }
}

criterion_group!(benches, bench_moo);
criterion_main!(benches);
