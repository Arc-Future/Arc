namespace Arc.Agent;
/// <summary>RFC 038 — M1 起枚举完整；禁另起同义枚举。</summary>
public enum AITurnState {
    Idle, Completing, StreamingTools, AwaitingTools, AwaitingHuman,
    DispatchingTools, Done, Failed, Cancelled
}