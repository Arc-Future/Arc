// RFC 038：AISkillSet — Skill 注册表（同 AIToolSet 链表纪律；禁反射）。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// Skill 注册表。按名注册/查找 <see cref="AISkill"/>；挂载到会话后由
/// <see cref="AIContextEngine"/> 统一组装（激活提示注入 + 工具聚合）。
/// </summary>
public class AISkillSet {
    private AISkillEntry _head;
    private int _count;

    public AISkillSet() {
        _head = null;
        _count = 0;
    }

    public int Count {
        get { return _count; }
    }

    /// <summary>注册 Skill（同名覆盖；null 忽略）。</summary>
    public void Add(AISkill skill) {
        if (skill == null || skill.Name == null || skill.Name == "") {
            return;
        }
        AISkillEntry prev = null;
        AISkillEntry cur = _head;
        while (cur != null) {
            if (cur.Skill != null && cur.Skill.Name == skill.Name) {
                cur.Skill = skill;
                return;
            }
            prev = cur;
            cur = cur.Next;
        }
        AISkillEntry entry = new AISkillEntry();
        entry.Skill = skill;
        if (prev == null) {
            _head = entry;
        } else {
            prev.Next = entry;
        }
        _count = _count + 1;
    }

    /// <summary>移除 Skill；存在返回 true。</summary>
    public bool Remove(string name) {
        if (name == null) { return false; }
        AISkillEntry prev = null;
        AISkillEntry cur = _head;
        while (cur != null) {
            if (cur.Skill != null && cur.Skill.Name == name) {
                if (prev == null) {
                    _head = cur.Next;
                } else {
                    prev.Next = cur.Next;
                }
                _count = _count - 1;
                return true;
            }
            prev = cur;
            cur = cur.Next;
        }
        return false;
    }

    /// <summary>按名查找 Skill；不存在返回 null。</summary>
    public AISkill Find(string name) {
        if (name == null) { return null; }
        AISkillEntry cur = _head;
        while (cur != null) {
            if (cur.Skill != null && cur.Skill.Name == name) {
                return cur.Skill;
            }
            cur = cur.Next;
        }
        return null;
    }

    /// <summary>全部 Skill 名（注册序）。</summary>
    public List<string> Names() {
        List<string> list = new List<string>();
        AISkillEntry cur = _head;
        while (cur != null) {
            if (cur.Skill != null) {
                list.Add(cur.Skill.Name);
            }
            cur = cur.Next;
        }
        return list;
    }

    /// <summary>深拷贝注册表（新实例 + 逐项注册；Skill 对象本身共享，只读）。</summary>
    public AISkillSet Clone() {
        AISkillSet c = new AISkillSet();
        List<string> names = this.Names();
        int n = names.Count;
        int i = 0;
        while (i < n) {
            AISkill s = this.Find(names[i]);
            if (s != null) {
                c.Add(s);
            }
            i = i + 1;
        }
        return c;
    }

    /// <summary>聚合全部 Skill 的能力工具为单一 AIToolSet（注册序；供 sandbox 附加调度）。</summary>
    public AIToolSet ToToolSet() {
        AIToolSet merged = new AIToolSet();
        List<string> names = this.Names();
        int n = names.Count;
        int i = 0;
        while (i < n) {
            AISkill s = this.Find(names[i]);
            AIToolSet st = s != null ? s.Tools : null;
            if (st != null) {
                st.ForEach((d: AIToolDescriptor, h: AIToolHandler) => {
                    merged.Add(d, h);
                });
            }
            i = i + 1;
        }
        return merged;
    }
}

/// <summary>Skill 注册表链表节点（内部；不暴露给开发者）。</summary>
internal class AISkillEntry {
    public AISkill Skill;
    public AISkillEntry Next;
    public AISkillEntry() {
        this.Skill = null;
        this.Next = null;
    }
}