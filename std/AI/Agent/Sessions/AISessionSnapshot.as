namespace Arc.Agent;
using Arc.Collections;
public class AISessionSnapshot {
    public string SessionId;
    public List<AIMessage> Transcript;
    public AITurnState Turn;
    internal AISessionBudget Budget;
    public AISessionSnapshot() {
        this.SessionId = "";
        this.Transcript = new List<AIMessage>();
        this.Turn = AITurnState.Idle;
        this.Budget = new AISessionBudget();
    }
}