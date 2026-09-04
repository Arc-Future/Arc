namespace UnitTest.Arc;

using Arc;
using Arc.IO;
using Arc.QIF;

/// <summary>
/// 文件 I/O 单元测试：覆盖 FileIo 示例。
/// </summary>
public class FileIoTests
{
    // ── Path 操作 ──

    [Fact]
    public void Path_Combine_TwoParts()
    {
        string combined = Path.Combine("dir", "file.txt");
        Assert.True(combined == "dir\\file.txt" || combined == "dir/file.txt");
    }

    [Fact]
    public void Path_GetFileName()
    {
        string name = Path.GetFileName("/tmp/test.txt");
        Assert.True(name == "test.txt");
    }

    [Fact]
    public void Path_GetExtension()
    {
        string ext = Path.GetExtension("file.txt");
        Assert.True(ext == ".txt");
    }

    [Fact]
    public void Path_GetFileNameWithoutExtension()
    {
        string name = Path.GetFileNameWithoutExtension("/tmp/test.txt");
        Assert.True(name == "test");
    }

    [Fact]
    public void Path_ChangeExtension()
    {
        Assert.True(Path.ChangeExtension("file.txt", ".md") == "file.md");
        Assert.True(Path.ChangeExtension("file.txt", "") == "file");
    }

    [Fact]
    public void Path_HasExtension()
    {
        Assert.True(Path.HasExtension("file.txt"));
        Assert.True(!Path.HasExtension("file"));
    }

    // ── File 文件系统 CRUD（真实 I/O；相对路径落在测试宿主 cwd；用完即删）──

    [Fact]
    public void File_WriteAllText_ReadAllText_Roundtrip()
    {
        string path = "qif_io_rw.txt";
        File.Delete(path); // 清理残留，保证从干净态开始
        bool wrote = File.WriteAllText(path, "hello arc");
        Assert.True(wrote);
        string content = File.ReadAllText(path);
        Assert.True(content == "hello arc");
        File.Delete(path);
        Assert.False(File.Exists(path));
    }

    [Fact]
    public void File_AppendAllText_Appends()
    {
        string path = "qif_io_append.txt";
        File.Delete(path);
        File.WriteAllText(path, "a");
        bool appended = File.AppendAllText(path, "b");
        Assert.True(appended);
        string content = File.ReadAllText(path);
        Assert.True(content == "ab");
        File.Delete(path);
    }

    [Fact]
    public void File_WriteAllBytes_ReadAllBytes_Roundtrip()
    {
        string path = "qif_io_bytes.txt";
        File.Delete(path);
        byte[] seed = [10, 20, 30, 40, 255];
        bool wrote = File.WriteAllBytes(path, seed);
        Assert.True(wrote);
        byte[] read = File.ReadAllBytes(path);
        Assert.True(read.Length == 5);
        Assert.True(read[0] == 10);
        Assert.True(read[4] == 255);
        File.Delete(path);
    }

    [Fact]
    public void File_ReadAllLines_Splits()
    {
        string path = "qif_io_lines.txt";
        File.Delete(path);
        File.WriteAllText(path, "alpha\nbeta\ngamma\n");
        string[] lines = File.ReadAllLines(path);
        Assert.True(lines.Length == 3);
        Assert.True(lines[0] == "alpha");
        Assert.True(lines[1] == "beta");
        Assert.True(lines[2] == "gamma");
        File.Delete(path);
    }

    [Fact]
    public void File_Exists_Delete()
    {
        string path = "qif_io_exists.txt";
        File.Delete(path);
        Assert.False(File.Exists(path));
        bool wrote = File.WriteAllText(path, "x");
        Assert.True(wrote);
        Assert.True(File.Exists(path));
        bool deleted = File.Delete(path);
        Assert.True(deleted);
        Assert.False(File.Exists(path));
    }

    [Fact]
    public void File_Copy_Move()
    {
        string src = "qif_io_copy_src.txt";
        string dst = "qif_io_copy_dst.txt";
        string moved = "qif_io_move_dst.txt";
        File.Delete(src);
        File.Delete(dst);
        File.Delete(moved);
        File.WriteAllText(src, "payload");
        Assert.True(File.Copy(src, dst));
        Assert.True(File.Exists(dst));
        Assert.True(File.Exists(src)); // Copy 不删除源
        Assert.True(File.Move(src, moved));
        Assert.False(File.Exists(src));
        Assert.True(File.Exists(moved));
        File.Delete(dst);
        File.Delete(moved);
    }

    // ── FileStream 同步读写/定位 ──

    [Fact]
    public void FileStream_Create_Write_Read()
    {
        string path = "qif_io_fs.txt";
        File.Delete(path);
        FileStream fs = FileStream.Create(path);
        Assert.True(fs.CanWrite);
        byte[] data = [1, 2, 3, 4];
        fs.Write(data, 0, 4);
        Assert.True(fs.Length == 4);
        fs.Dispose();
        Assert.True(File.Exists(path));
        FileStream rs = FileStream.OpenRead(path);
        Assert.True(rs.CanRead);
        byte[] buf = new byte[4];
        int n = rs.Read(buf, 0, 4);
        Assert.True(n == 4);
        Assert.True(buf[0] == 1);
        Assert.True(buf[3] == 4);
        rs.Dispose();
        File.Delete(path);
    }

    [Fact]
    public void FileStream_Read_Eof_ReturnsZero()
    {
        string path = "qif_io_fs_eof.txt";
        File.Delete(path);
        FileStream fs = FileStream.Create(path);
        byte[] data = [7, 8];
        fs.Write(data, 0, 2);
        fs.Dispose();
        FileStream rs = FileStream.OpenRead(path);
        byte[] buf = new byte[2];
        int first = rs.Read(buf, 0, 2);
        Assert.True(first == 2);
        int second = rs.Read(buf, 0, 2);
        Assert.True(second == 0);
        rs.Dispose();
        File.Delete(path);
    }

    // ── Directory ──

    [Fact]
    public void Directory_CreateExists_GetFiles_Delete()
    {
        string dir = "qif_io_dir";
        // 尽力清理历史残留（目录非空时删除会失败，不在此断言；流程断言放在 Create 之后）
        File.Delete(dir + "\\a.txt");
        File.Delete(dir + "\\b.txt");
        Directory.Delete(dir);
        Assert.True(Directory.CreateDirectory(dir));
        Assert.True(Directory.Exists(dir));
        string f1 = dir + "\\a.txt";
        string f2 = dir + "\\b.txt";
        File.WriteAllText(f1, "1");
        File.WriteAllText(f2, "2");
        string[] files = Directory.GetFiles(dir);
        Assert.True(files.Length == 2);
        // 用 GetFiles 返回的真实路径删除，避免手拼路径与完整路径不一致
        int k = 0;
        while (k < files.Length) {
            Assert.True(File.Delete(files[k]));
            k = k + 1;
        }
        Assert.True(Directory.Delete(dir));
        Assert.False(Directory.Exists(dir));
    }

    [Fact]
    public void Directory_GetDirectories_ListsSubdirs()
    {
        string parent = "qif_io_parent";
        string childA = parent + "\\subA";
        string childB = parent + "\\subB";
        Directory.Delete(parent);
        Directory.CreateDirectory(parent);
        Directory.CreateDirectory(childA);
        Directory.CreateDirectory(childB);
        string[] subs = Directory.GetDirectories(parent);
        Assert.True(subs.Length == 2);
        Directory.Delete(parent);
    }
}
