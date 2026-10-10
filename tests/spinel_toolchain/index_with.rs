//! The native counterpart of emitted CRuby's `Enumerable#index_with` test.

use super::index_with;

#[test]
#[ignore = "requires Spinel; run in its CI lane"]
fn index_with_block_form_runs_natively() {
    let run = index_with::overlay().run_spinel(index_with::ASSERTIONS);
    run.assert_passes();
    assert!(
        run.stdout.contains("index_with contract passed"),
        "{}",
        run.stdout
    );
}
