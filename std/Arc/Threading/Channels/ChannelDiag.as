// ChannelDiag — 通道诊断计数器（丢失唤醒取证，定位后整体回收）。
namespace Arc.Threading.Channels;

/// <summary>
/// 通道操作计数器（非线程安全，诊断读取容忍竞争）：冻结现场定位
/// 「哪类通道操作未发生」。心跳任务周期打印快照。
/// 临时取证设施：定位残余后随计数点整体回收。
/// </summary>
public class ChannelDiag {
    private static int _tryWriteDirect;
    private static int _tryWriteBuffered;
    private static int _tryWriteRejected;
    private static int _writeEnqueueRegistered;
    private static int _writeEnqueueSync;
    private static int _readEnqueueRegistered;
    private static int _readEnqueueSync;
    private static int _serveReader;
    private static int _admitWriters;
    private static int _completeRecheck;
    private static int _completeWriterFail;
    private static int _settle;
    private static int _dequeue;
    private static int _tryReadHit;

    public static void CountTryWriteDirect() {
        _tryWriteDirect = _tryWriteDirect + 1;
    }

    public static void CountTryWriteBuffered() {
        _tryWriteBuffered = _tryWriteBuffered + 1;
    }

    public static void CountTryWriteRejected() {
        _tryWriteRejected = _tryWriteRejected + 1;
    }

    public static void CountWriteEnqueueRegistered() {
        _writeEnqueueRegistered = _writeEnqueueRegistered + 1;
    }

    public static void CountWriteEnqueueSync() {
        _writeEnqueueSync = _writeEnqueueSync + 1;
    }

    public static void CountReadEnqueueRegistered() {
        _readEnqueueRegistered = _readEnqueueRegistered + 1;
    }

    public static void CountReadEnqueueSync() {
        _readEnqueueSync = _readEnqueueSync + 1;
    }

    public static void CountServeReader() {
        _serveReader = _serveReader + 1;
    }

    public static void CountAdmitWriters() {
        _admitWriters = _admitWriters + 1;
    }

    public static void CountCompleteRecheck() {
        _completeRecheck = _completeRecheck + 1;
    }

    public static void CountCompleteWriterFail() {
        _completeWriterFail = _completeWriterFail + 1;
    }

    public static void CountSettle() {
        _settle = _settle + 1;
    }

    public static void CountDequeue() {
        _dequeue = _dequeue + 1;
    }

    public static void CountTryReadHit() {
        _tryReadHit = _tryReadHit + 1;
    }

    public static string Snapshot() {
        return "direct=" + _tryWriteDirect + " buf=" + _tryWriteBuffered
            + " rej=" + _tryWriteRejected
            + " wReg=" + _writeEnqueueRegistered + " wSync=" + _writeEnqueueSync
            + " rReg=" + _readEnqueueRegistered + " rSync=" + _readEnqueueSync
            + " serve=" + _serveReader + " admit=" + _admitWriters
            + " recheck=" + _completeRecheck + " wfail=" + _completeWriterFail
            + " settle=" + _settle
            + " dq=" + _dequeue + " trh=" + _tryReadHit;
    }
}
