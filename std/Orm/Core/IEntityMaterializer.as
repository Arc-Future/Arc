// L3 骨架：IEntityMaterializer — 物化器接口。
namespace Arc.Orm;

using Arc.Data;

/// <summary>
/// 实体物化器接口——从数据读取器构造实体实例。
///
/// codegen 为每个实体类型生成专用实现，零反射、零 IL Emit。
/// </summary>
public interface IEntityMaterializer {
    /// <summary>从数据读取器物化实体。</summary>
    /// <param name="reader">数据读取器（已定位到当前行）。</param>
    /// <returns>物化的实体实例。</returns>
    object Materialize(IDataReader reader);
}
