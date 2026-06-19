use gauge::tui::data::TimeWindow;

#[test]
fn time_windows_cycle_and_map_to_dsl() {
    assert_eq!(TimeWindow::H1.last(), "1h");
    assert_eq!(TimeWindow::D30.next(), TimeWindow::H1);
    assert_eq!(
        TimeWindow::H24.granularity(),
        gauge_query::Granularity::Hour
    );
    assert_eq!(TimeWindow::D7.granularity(), gauge_query::Granularity::Day);
}
