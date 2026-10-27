"""应用独立预审要求的最小能力门；须先保留旧候选失败证据。"""
from pathlib import Path

base = Path('packages/engine/src')
p = base / 'runtime/diagnostics.rs'
s = p.read_text()
s = s.replace('    Quote(DeskQuote),', '''    Quote(DeskQuote),
    /// 中间 slice 尚未接通的真实路径；不能生成看似完整的报告。
    Unsupported { detail: &'static str },''', 1)
p.write_text(s)
p = base / 'runtime/desk.rs'
s = p.read_text()
needle = '''                before: causal_before,
            });'''
repl = needle + '''
            if is_market || phase != DeskPhase::Continuous {
                self.note_causal(super::diagnostics::DeskCausalKind::Unsupported {
                    detail: "Market或非连续申报生命周期尚未接通",
                });
            }'''
assert needle in s
s = s.replace(needle, repl, 1)
needle = '''        // 实体互换：买方得股票、卖方得净额。'''
s = s.replace(needle, '''        #[cfg(feature = "simulation-diagnostics")]
        self.note_causal(super::diagnostics::DeskCausalKind::Unsupported {
            detail: "真实Filled/Execution生命周期尚未接通",
        });
''' + needle, 1)
needle = '''            self.note_terminated(&o, super::protocol::LifecycleKind::Expired);'''
s = s.replace(needle, needle + '''
            #[cfg(feature = "simulation-diagnostics")]
            self.note_causal(super::diagnostics::DeskCausalKind::Unsupported {
                detail: "真实Expired终止生命周期尚未接通",
            });''', 1)
p.write_text(s)
p = base / 'runtime/drive.rs'
s = p.read_text()
needle = '''    /// 消费本拍 sideband；report_log 不再永久保存第二份诊断历史。'''
s = s.replace(needle, '''    /// Drain 的能力门只读现有权威发布，不新增查询往返或伪造终止事实。
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn ensure_causal_day_end_supported(&self) -> Result<(), String> {
        let published = self.world.publication().load();
        if published.desks.iter().any(|desk| desk.resting_count > 0) {
            return Err("causal DayEnd真实余单终止尚未接通".to_owned());
        }
        Ok(())
    }

''' + needle, 1)
for entry in [
    '    pub fn day_end(&mut self, session: &mut GameSession) -> Result<(), String> {',
    '''    pub fn day_end_with_runtime_market_day(
        &mut self,
        session: &mut GameSession,
    ) -> Result<(), String> {''',
    '''    pub fn world_day_end_for_step(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Vec<crate::session::personal_trade_history::PendingTradeReceipt>, String>
    {''',
]:
    assert entry in s
    s = s.replace(entry, entry + '''
        #[cfg(feature = "simulation-diagnostics")]
        self.ensure_causal_day_end_supported()?;''', 1)
p.write_text(s)
p = base / 'session/causal.rs'
s = p.read_text()
needle = '''        let mut kinds = Vec::new();'''
s = s.replace(needle, '''        if self.tick().is_multiple_of(self.state.setup.ticks_per_day) {
            driver.ensure_causal_day_end_supported().map_err(fail)?;
        }
''' + needle, 1)
needle = '''                DeskCausalKind::Quote(raw) => kinds.push(CausalFactKind::Quote(quote(raw))),'''
s = s.replace(needle, needle + '''
                DeskCausalKind::Unsupported { detail } => return Err(fail(detail.to_owned())),''', 1)
p.write_text(s)
