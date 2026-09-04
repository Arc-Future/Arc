// RFC 049 M-C: Arc.Net.Grpc — gRPC 方法定义（服务名/方法名 → 调用路径）。
//
// 无代码生成器：服务方法以显式 MethodDefinition 注册（对齐 RFC 049 §1.1 无
// protoc 编译链）。`FullName` 即 gRPC HTTP/2 的 `:path = /package.Service/Method`。
//
// 访问权限：public（用户面契约）。内部实现仅聚合两个名字字段。

namespace Arc.Net.Grpc;

/// <summary>gRPC 方法定义：服务名 + 方法名，`FullName` 供 `:path` 使用。</summary>
public class GrpcMethodDefinition {
    public GrpcMethodDefinition(string serviceName, string methodName) {
        ServiceName = serviceName;
        MethodName = methodName;
    }

    /// <summary>服务全名（如 "hello.HelloService"）。</summary>
    public string ServiceName { get; }

    /// <summary>方法名（如 "SayHello"）。</summary>
    public string MethodName { get; }

    /// <summary>gRPC 调用路径（`:path` = "/服务全名/方法名"）。</summary>
    public string FullName { get { return "/" + ServiceName + "/" + MethodName; } }
}
