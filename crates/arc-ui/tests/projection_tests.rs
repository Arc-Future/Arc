//! RFC 027 M-U2 编译期投影模块测试：状态空间枚举 + 投影表 + 索引公式。
//!
//! 全部为手算对照（§11.5 确定性规则：维度独立求值 → 档位区间唯一 →
//! 特异性次序 → 兜底）。

use arc_ui::{build_projection_spec, encode_state, DimKind, Parser};

fn spec_of(src: &str) -> arc_ui::ProjectionSpec {
    let doc = Parser::parse(src).expect("parse");
    build_projection_spec(&doc).expect("build spec")
}

/// §11.3 Spacing.Page：Tier sm/md/lg + 兜底。
/// 维度 = [Tier]，card = 3+1 = 4，num_states = 4。
/// 表 = [sm→8, md→16, lg→24, no-match→16]。
#[test]
fn projection_tier_token_table() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Spacing.Page">
            <Match Tier="sm" Value="8" />
            <Match Tier="md" Value="16" />
            <Match Tier="lg" Value="24" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
    <StackPanel Padding="{Token Spacing.Page}" />
</Window>"##,
    );
    assert_eq!(spec.dims.len(), 1);
    assert_eq!(spec.dims[0].kind, DimKind::Tier);
    assert_eq!(spec.dims[0].card, 4);
    assert_eq!(spec.num_states, 4);
    assert_eq!(spec.tokens.len(), 1);
    let t = &spec.tokens[0];
    assert_eq!(t.intervals, 1);
    // 手算：sm→8, md→16, lg→24, 未引用档位（no-match）→ 兜底 16
    assert_eq!(t.table, vec![8.0, 16.0, 24.0, 16.0]);
    assert_eq!(t.units, vec![0, 0, 0, 0]);
}

/// 死组合剔除：只有 Media="dark" 条件的 Token + 只有 Tier 条件的 Token
/// → 状态空间只含实际用到的维度（Tier + Media，无 Idiom/Density）。
#[test]
fn projection_dead_combo_elimination() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="A">
            <Match Media="dark" Value="1" />
            <Match Value="0" />
        </Double>
        <Double x:Key="B">
            <Match Tier="sm" Value="2" />
            <Match Tier="lg" Value="4" />
            <Match Value="3" />
        </Double>
    </Window.Resources>
    <StackPanel Padding="{Token A}" />
</Window>"##,
    );
    // 维度序：Idiom → Tier → Density → Media（首见序），只含用到的
    assert_eq!(spec.dims.len(), 2);
    assert_eq!(spec.dims[0].kind, DimKind::Tier);
    assert_eq!(spec.dims[1].kind, DimKind::Media);
    assert_eq!(spec.dims[1].coordinate.as_deref(), Some("dark"));
    // Tier card=3 (sm/lg/no-match) × Media card=2 (true/no-match) = 6
    assert_eq!(spec.num_states, 6);
    let a = &spec.tokens[0];
    let b = &spec.tokens[1];
    // 状态序：tier_major × media_minor（tier stride=2, media stride=1）
    // A：只在 Media 维变化（dark 命中 1，否则兜底 0）
    assert_eq!(a.table, vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    // B：只在 Tier 维变化（sm=2, lg=4, no-match=3）；Media 维恒定
    assert_eq!(b.table, vec![2.0, 2.0, 4.0, 4.0, 3.0, 3.0]);
}

/// 特异性次序：属性多者胜；同数量按 Media > Density > Idiom > Tier。
#[test]
fn projection_specificity_ordering() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="C">
            <Match Media="dark" Idiom="Desktop" Value="1" />
            <Match Media="dark" Density="compact" Value="2" />
            <Match Media="dark" Value="3" />
            <Match Value="4" />
        </Double>
    </Window.Resources>
</Window>"##,
    );
    // 维度序：Idiom → Tier → Density → Media
    assert_eq!(spec.dims.len(), 3);
    assert_eq!(spec.dims[0].kind, DimKind::Idiom);
    assert_eq!(spec.dims[1].kind, DimKind::Density);
    assert_eq!(spec.dims[2].kind, DimKind::Media);
    // Idiom card=2 (Desktop/no) × Density card=2 × Media card=2 = 8
    assert_eq!(spec.num_states, 8);
    let t = &spec.tokens[0];
    // 手算（状态序：idiom_major × density × media_minor）：
    //   (D,compact,dark)=2 [Media+Density 2 条件，Density>Idiom 胜出]
    //   (D,compact,no)=4 兜底
    //   (D,no,dark)=1 [Media+Idiom 2 条件]
    //   (D,no,no)=4 兜底
    //   (no,compact,dark)=2 [Media+Density]
    //   (no,compact,no)=4 兜底
    //   (no,no,dark)=3 [仅 Media 1 条件]
    //   (no,no,no)=4 兜底
    assert_eq!(t.table, vec![2.0, 4.0, 1.0, 4.0, 2.0, 4.0, 3.0, 4.0]);
}

/// 断点 Token：MinWidth/MaxWidth → 区间维（唯一连续运行期坐标面）。
#[test]
fn projection_breakpoint_token_intervals() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Panel.W">
            <Match MinWidth="600" MaxWidth="959" Value="1" />
            <Match MinWidth="960" Value="2" />
            <Match Value="0" />
        </Double>
    </Window.Resources>
</Window>"##,
    );
    let t = &spec.tokens[0];
    // 阈值来自全部断点谓词：[600(Min), 959(Max), 960(Min)] → 升序去重
    assert_eq!(t.thresholds, vec![600.0, 959.0, 960.0]);
    assert_eq!(t.intervals, 4);
    // 无静态维度 → num_states=1
    assert_eq!(spec.num_states, 1);
    // 区间：(-inf,600)→兜底0；[600,959)→Match1→1；[959,960)→无覆盖→兜底0；[960,+inf)→Match2→2
    assert_eq!(t.table, vec![0.0, 1.0, 0.0, 2.0]);
}

/// 混合：静态（Tier）+ 断点（MinWidth）同一 Token。
/// 表长度 = num_states × intervals。
#[test]
fn projection_mixed_static_breakpoint() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Mix">
            <Match Tier="sm" MinWidth="600" Value="10" />
            <Match Tier="sm" Value="1" />
            <Match Tier="lg" Value="2" />
            <Match Value="0" />
        </Double>
    </Window.Resources>
</Window>"##,
    );
    let t = &spec.tokens[0];
    // Tier card=3, 阈值 [600] → intervals=2
    assert_eq!(spec.num_states, 3);
    assert_eq!(t.intervals, 2);
    // 手算展开（state × interval）：
    // state0(sm):  [600,+inf) 命中 MinWidth Match(10)；(-inf,600) 命中 sm(1) → [1, 10]
    // state1(lg):  2（区间无关）
    // state2(no-match): 0（兜底）
    assert_eq!(t.table, vec![1.0, 10.0, 2.0, 2.0, 0.0, 0.0]);
}

/// `<Adaptive>` 子树谓词收集（MinWidth/MaxWidth）。
#[test]
fn projection_adaptive_collection() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <StackPanel>
        <Adaptive MinWidth="600"><Grid /><TextBlock Text="List" /></Adaptive>
        <Adaptive MaxWidth="599"><TextBlock Text="Tap" /></Adaptive>
    </StackPanel>
</Window>"##,
    );
    assert!(spec.has_adaptives);
    assert_eq!(spec.adaptives.len(), 2);
    assert_eq!(spec.adaptives[0].min_width, Some(600.0));
    assert_eq!(spec.adaptives[0].max_width, None);
    assert_eq!(spec.adaptives[1].max_width, Some(599.0));
}

/// 参数化媒体坐标：MediaValue 字面量进入 Media 维度值集。
#[test]
fn projection_parameterized_media() {
    let spec = spec_of(
        r##"<Application x:Class="Ns.App">
    <Application.Media>
        <Media Name="font-scale" Type="Ratio" />
    </Application.Media>
    <Application.Resources>
        <Double x:Key="Text.Body">
            <Match Media="font-scale" MediaValue="1.3" Value="20" />
            <Match Value="16" />
        </Double>
    </Application.Resources>
</Application>"##,
    );
    assert_eq!(spec.dims.len(), 1);
    assert_eq!(spec.dims[0].kind, DimKind::Media);
    assert_eq!(spec.dims[0].coordinate.as_deref(), Some("font-scale"));
    assert_eq!(spec.dims[0].values, vec![smol_str::SmolStr::from("1.3")]);
    assert_eq!(spec.num_states, 2);
    let t = &spec.tokens[0];
    assert_eq!(t.table, vec![20.0, 16.0]);
}

/// lpx 检测：Match Value 携带 lpx 单位。
#[test]
fn projection_lpx_detection() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Spacing.Hero">
            <Match Tier="lg" Value="24lpx" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##,
    );
    assert!(spec.has_lpx);
    let t = &spec.tokens[0];
    // Tier 值 [lg] → card 2：lg→24(lpx), no-match→16(vp)
    assert_eq!(t.table, vec![24.0, 16.0]);
    assert_eq!(t.units, vec![3, 0]);
}

/// `{Token}` 别名 base：`<Double x:Key="A" Value="{Token B}"/>`。
#[test]
fn projection_token_alias_base() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="B" Value="42" />
        <Double x:Key="A" Value="{Token B}" />
    </Window.Resources>
</Window>"##,
    );
    let b = spec.tokens.iter().find(|t| t.name == "B").unwrap();
    let a = spec.tokens.iter().find(|t| t.name == "A").unwrap();
    assert_eq!(a.table, b.table);
    assert_eq!(a.table, vec![42.0]);
}

/// 索引公式：encode/decode 往返一致（§11.5 索引公式）。
#[test]
fn projection_index_formula_roundtrip() {
    let spec = spec_of(
        r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Media="dark" Value="1" />
            <Match Tier="sm" Value="2" />
            <Match Value="0" />
        </Double>
    </Window.Resources>
</Window>"##,
    );
    assert_eq!(spec.dims.len(), 2);
    // 遍历全状态空间：decode 后 re-encode 应还原
    for s in 0..spec.num_states {
        let coords = decode_via_spec(&spec, s);
        assert_eq!(encode_state(&spec.dims, &coords), s);
    }
}

fn decode_via_spec(spec: &arc_ui::ProjectionSpec, state: usize) -> Vec<usize> {
    let mut coords = vec![0usize; spec.dims.len()];
    let mut rem = state;
    for (i, d) in spec.dims.iter().enumerate() {
        coords[i] = (rem / d.stride) % d.card;
        rem %= d.stride;
    }
    coords
}
