const fs = require("fs");
const lines = fs.readFileSync("crates/mir/src/lib.rs", "utf8").split("\n");
const find = (s) => lines.findIndex((l) => l.includes(s));
const join = (a, b) => lines.slice(a, b).join("\n") + "\n";

const iLocalId = find("pub struct LocalId");
const iLowerCtx = find("struct LowerCtx");
const iMirBuilder = find("pub struct MirBuilder");
const iTests = find("#[cfg(test)]");

fs.writeFileSync(
  "crates/mir/src/types.rs",
  join(iLocalId, iMirBuilder)
);

fs.writeFileSync(
  "crates/mir/src/lower.rs",
  join(iLowerCtx, iTests - 1)
);

fs.writeFileSync(
  "crates/mir/src/lib.rs",
  [
    "//! Mid-level IR for Arc.",
    "//!",
    "//! LINQ / ExpressionTree lowering follows the compile-time expansion contract",
    "//! (`docs/rfc/011-expression-trees-query.md`, RFC 011):",
    "//! - `LinqChain` + `LinqForeach`: Enumerable path; consumed by codegen for specialized loops.",
    "//! - `ExpressionTreeConst`: Queryable path; tree is input to codegen rodata emission only—",
    "//!   not an instruction to interpret the tree at user-program runtime.",
    "",
    "mod types;",
    "mod lower;",
    "",
    "pub use types::*;",
    "pub use lower::lower_module;",
    "",
    join(iTests, lines.length),
  ].join("\n")
);

console.log("mir split ok");
