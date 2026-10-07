interface MechanismSwitchesInputProps {
  readonly rightsOfferingEnabled: boolean;
  readonly issuerRepurchaseEnabled: boolean;
  readonly onRightsOfferingChange: (value: boolean) => void;
  readonly onIssuerRepurchaseChange: (value: boolean) => void;
}

/**
 * 新局公司行为机制开关（2026-10-07 产品决策，ADR-0038/0039）：
 * 配股／增发与回购是两个独立开关，默认关闭；仅对新游戏生效，
 * 随存档严格固化（同局不可切换）。
 *
 * NPC 认购策略选择器保持 TODO：当前只实现默认策略「足额认购」
 * （NPC 以本人真实现金足额认购，不足部分放弃并如实记录）；
 * 其他策略（如按真实策略判断）engine 侧显式拒绝，前端仅占位标注，
 * 不提供会静默降级的选择入口。
 */
export function MechanismSwitchesInput({ rightsOfferingEnabled, issuerRepurchaseEnabled, onRightsOfferingChange, onIssuerRepurchaseChange }: MechanismSwitchesInputProps) {
  return (
    <fieldset className="mechanism-switches-input" title="仅对新游戏生效。开关随存档严格固化，开局后不可修改。">
      <legend>新局公司行为机制</legend>
      <label>
        <input
          type="checkbox"
          name="rights-offering-enabled"
          checked={rightsOfferingEnabled}
          onChange={(event) => onRightsOfferingChange(event.currentTarget.checked)}
        />
        启用配股／增发（默认关闭）
      </label>
      <label>
        <input
          type="checkbox"
          name="issuer-repurchase-enabled"
          checked={issuerRepurchaseEnabled}
          onChange={(event) => onIssuerRepurchaseChange(event.currentTarget.checked)}
        />
        启用发行人回购（默认关闭，独立开关）
      </label>
      <small>
        关闭时对应机制不触发，显式调用被拒绝（错误指明本局未启用）。
        NPC 认购策略当前仅实现「默认足额认购」；其他策略属后续批次（TODO，未实现）。
      </small>
    </fieldset>
  );
}
