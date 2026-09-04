//! L3 Illusory 引擎 M1 骨架冒烟批（RFC 049 references 02 §9 门禁）。
//!
//! Arc.Illusory 为独立 std 子库（std/Illusory/Core），经 build_and_run_batch_with_deps
//! 以 extra_deps `("Arc.Illusory", "Illusory/Core")` 注入依赖后编译运行。
//! 覆盖：固定步长切分/单调、Actor 生命周期、组件仓库、GameplayTags 值语义。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{
    batch_case_result, build_and_run_batch_with_deps, BatchCase,
};

#[test]
fn illusory_simulation_batch() {
    // 固定步长切分（25ms 步长 / 一帧 80ms → 恰 3 tick，余 5ms 累计到下一帧 → 再 +1 tick）
    // 与步印单调递增、DeltaTime 恒定。
    let results = build_and_run_batch_with_deps(
        "illusory_simulation",
        &[
            BatchCase {
                name: "sim_fixed_step",
                src: r#"using Arc;
using Arc.Collections;
using Arc.Illusory;
using Arc.Math;

IReadOnlyList<object> NoServices() {
    return new ReadOnlyCollection<object>(new List<object>());
}

bool InRange(float v, float lo, float hi) {
    return v > lo && v < hi;
}

void Main() {
    IWorld w = Worlds.Create(new WorldOptions(25.0f, NoServices()));
    // 一帧 80ms @ 25ms 固定步长 → 恰 3 个 tick，余 5ms 累计到下帧。
    w.Update(80.0f);
    SimulationTick t = w.CurrentTick;
    if (t.Step != 3) { Console.WriteLine("ARC_CASE:sim_fixed_step:FAIL:step=" + t.Step); return; }
    if (!InRange(t.DeltaTime, 0.0249f, 0.0251f)) { Console.WriteLine("ARC_CASE:sim_fixed_step:FAIL:delta=" + t.DeltaTime); return; }
    float expectedTime = 3.0f * 0.025f;
    if (!InRange(t.Time, expectedTime - 0.0001f, expectedTime + 0.0001f)) { Console.WriteLine("ARC_CASE:sim_fixed_step:FAIL:time=" + t.Time); return; }
    // 余量 5ms + 新增 20ms = 25ms → 恰好 +1 tick，Step 单调到 4。
    w.Update(20.0f);
    SimulationTick t2 = w.CurrentTick;
    if (t2.Step != 4) { Console.WriteLine("ARC_CASE:sim_fixed_step:FAIL:step2=" + t2.Step); return; }
    if (!InRange(t2.DeltaTime, 0.0249f, 0.0251f)) { Console.WriteLine("ARC_CASE:sim_fixed_step:FAIL:delta2=" + t2.DeltaTime); return; }
    Console.WriteLine("ARC_CASE:sim_fixed_step:PASS");
}
"#,
            },
            BatchCase {
                name: "sim_monotonic",
                src: r#"using Arc;
using Arc.Illusory;

void Main() {
    IWorld w = Worlds.Create(new WorldOptions(10.0f, NoServices()));
    float[] frames = [25.0f, 30.0f, 15.0f, 40.0f, 20.0f, 35.0f, 10.0f, 45.0f, 55.0f, 25.0f, 30.0f, 20.0f, 60.0f, 40.0f, 18.0f];
    int prev = 0;
    float delta = 0.0f;
    int failed = 0;
    int i = 0;
    while (i < 15) {
        w.Update(frames[i]);
        SimulationTick t = w.CurrentTick;
        if (t.Step <= prev) { failed = 1; }
        prev = t.Step;
        if (i == 0) { delta = t.DeltaTime; }
        if (!(t.DeltaTime > delta - 0.0001f && t.DeltaTime < delta + 0.0001f)) { failed = 2; }
        i = i + 1;
    }
    if (failed != 0) { Console.WriteLine("ARC_CASE:sim_monotonic:FAIL:code=" + failed + " step=" + prev); return; }
    Console.WriteLine("ARC_CASE:sim_monotonic:PASS");
}
"#,
            },
        ],
        &[("Arc.Illusory", "Illusory/Core")],
    );

    for res in &results {
        eprintln!("=== {} passed={:?} err={:?}\n{}", res.name, res.passed, res.error, res.stdout);
    }

    let r = batch_case_result(&results, "sim_fixed_step");
    assert!(r.passed, "sim_fixed_step failed: {:?} stdout: {}", r.error, r.stdout);

    let r = batch_case_result(&results, "sim_monotonic");
    assert!(r.passed, "sim_monotonic failed: {:?} stdout: {}", r.error, r.stdout);
}

#[test]
fn illusory_actor_batch() {
    // Actor 生命周期：Spawn 分配单调 Id、TryGet 命中/未命中、Destroy 后不可见。
    let results = build_and_run_batch_with_deps(
        "illusory_actor",
        &[
            BatchCase {
                name: "actor_lifecycle",
                src: r#"using Arc;
using Arc.Illusory;
using Arc.Math;

void Main() {
    IWorld w = Worlds.Create(new WorldOptions());
    Actor a = w.SpawnActor(Transform.Identity);
    ActorId id = a.Id;
    if (id.Value < 1) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:zero_id"); return; }
    Actor got = null;
    if (!w.TryGetActor(id, out got)) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:miss_exists"); return; }
    if (got.Id.Value != id.Value) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:id_mismatch"); return; }
    if (w.TryGetActor(ActorId.None, out got)) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:none_hit"); return; }
    if (!w.TryDestroyActor(id)) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:destroy"); return; }
    if (w.TryGetActor(id, out got)) { Console.WriteLine("ARC_CASE:actor_lifecycle:FAIL:exists_after_destroy"); return; }
    Console.WriteLine("ARC_CASE:actor_lifecycle:PASS");
}
"#,
            },
            BatchCase {
                name: "component_store",
                src: r#"using Arc;
using Arc.Illusory;
using Arc.Math;

class HealthComponent : IComponent {
    public int HitPoints;
}

void Main() {
    IWorld w = Worlds.Create(new WorldOptions());
    Actor a = w.SpawnActor(Transform.Identity);
    HealthComponent hp = new HealthComponent();
    hp.HitPoints = 100;
    w.AddComponent(a.Id, hp);
    HealthComponent outC = null;
    if (!w.TryGetComponent(a.Id, out outC)) { Console.WriteLine("ARC_CASE:component_store:FAIL:get"); return; }
    if (outC.HitPoints != 100) { Console.WriteLine("ARC_CASE:component_store:FAIL:value"); return; }
    if (!w.RemoveComponent(a.Id, hp)) { Console.WriteLine("ARC_CASE:component_store:FAIL:remove"); return; }
    if (w.TryGetComponent(a.Id, out outC)) { Console.WriteLine("ARC_CASE:component_store:FAIL:exists_after_remove"); return; }
    Console.WriteLine("ARC_CASE:component_store:PASS");
}
"#,
            },
        ],
        &[("Arc.Illusory", "Illusory/Core")],
    );

    for res in &results {
        eprintln!("=== {} passed={:?} err={:?}\n{}", res.name, res.passed, res.error, res.stdout);
    }

    let r = batch_case_result(&results, "actor_lifecycle");
    assert!(r.passed, "actor_lifecycle failed: {:?} stdout: {}", r.error, r.stdout);

    let r = batch_case_result(&results, "component_store");
    assert!(r.passed, "component_store failed: {:?} stdout: {}", r.error, r.stdout);
}

#[test]
fn illusory_gameplay_tags_batch() {
    // GameplayTags：不可变值语义——Add/Remove 返回新实例，原实例不变；Overlaps 判交。
    let results = build_and_run_batch_with_deps(
        "illusory_gameplay_tags",
        &[
            BatchCase {
                name: "tags_immutable",
                src: r#"using Arc;
using Arc.Illusory;

void Main() {
    GameplayTags empty = default(GameplayTags);
    if (!empty.IsEmpty) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:not_empty"); return; }
    GameplayTags t = empty.Add("creature");
    if (!t.Has("creature")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:has"); return; }
    if (empty.Has("creature")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:mutated_source"); return; }
    if (!empty.IsEmpty) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:source_modified"); return; }
    GameplayTags t2 = t.Add("attack");
    if (!t2.Has("attack")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:attack"); return; }
    if (!t2.Overlaps(t)) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:overlaps"); return; }
    GameplayTags removed = t2.Remove("creature");
    if (removed.Has("creature")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:removed_tag"); return; }
    if (!removed.Has("attack")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:keep_tag"); return; }
    if (!t2.Has("creature")) { Console.WriteLine("ARC_CASE:tags_immutable:FAIL:source2_modified"); return; }
    Console.WriteLine("ARC_CASE:tags_immutable:PASS");
}
"#,
            },
        ],
        &[("Arc.Illusory", "Illusory/Core")],
    );

    let r = batch_case_result(&results, "tags_immutable");
    assert!(r.passed, "tags_immutable failed: {:?} stdout: {}", r.error, r.stdout);
}