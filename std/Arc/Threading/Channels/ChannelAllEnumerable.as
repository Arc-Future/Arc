// ChannelAllEnumerable<T> — ReadAllAsync 异步序列（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;
using Arc.Collections;

/// <summary>
/// ReadAllAsync 的异步序列——IAsyncEnumerable 面：GetAsyncEnumerator 每次
/// 发放全新游标（可多次枚举，各游标独立驱动读取委托）。取消令牌经
/// ReadAllAsync 绑定进读取委托（消费循环经 MoveNextAsync/Current 手写拉取，
/// await foreach 脱糖待 RFC 008 落地）。
/// 持 Func 委托而非读端引用：泛型类交叉引用会令单态化注册中断
///（ChannelReader ⇄ 枚举器环），Func 为 Builtin 泛型面无此环。
/// 序列终结：ChannelClosedException 归为序列结束；终结 error / 取消原样传播。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class ChannelAllEnumerable<T> : IAsyncEnumerable<T> {
    private Func<Task<T>> _readOne;

    /// <summary>以读取委托创建序列（委托捕获读端与取消令牌）。</summary>
    /// <param name="readOne">单次读取委托。</param>
    public ChannelAllEnumerable(Func<Task<T>> readOne) {
        _readOne = readOne;
    }

    /// <summary>发放全新异步枚举器游标。</summary>
    /// <param name="cancellationToken">取消令牌（经 ReadAllAsync 绑定，此处不重复生效）。</param>
    /// <returns>异步枚举器实例。</returns>
    public IAsyncEnumerator<T> GetAsyncEnumerator(CancellationToken cancellationToken) {
        return new ChannelAllEnumerator<T>(_readOne);
    }
}
