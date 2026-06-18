use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::config::PanelSpec;
use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};
use crate::tui::theme::Theme;

pub struct TopN;

impl TopN {
    pub fn from_spec(_spec: &PanelSpec) -> Result<Self, String> {
        Err("top_n not implemented yet".into())
    }
}

impl Panel for TopN {
    fn title(&self) -> String {
        "top_n".into()
    }
    fn data_requests(&self, _ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![]
    }
    fn render(&self, _f: &mut Frame, _a: Rect, _c: &PanelCtx, _r: &ResultMap, _t: &Theme) {}
}
