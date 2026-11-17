// Drags files into Vibez the way Explorer does: an OLE DoDragDrop from a
// window in this process, moved by synthetic mouse input.
//
// Usage: drag_windows.exe <vibez pid> <screenshot folder> <interface scale> <step>...
// where each step is four arguments, <file or "click"> <x> <y> <name>, with
// x and y in Vibez's logical pixels from the top left of its client area.

using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;

static class DragWindows
{
    const uint LeftDown = 0x0002;
    const uint LeftUp = 0x0004;
    const uint Move = 0x0001;
    const uint Absolute = 0x8000;

    [StructLayout(LayoutKind.Sequential)]
    struct Point
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    static extern bool SetProcessDPIAware();

    [DllImport("user32.dll")]
    static extern bool ClientToScreen(IntPtr window, ref Point point);

    [DllImport("user32.dll")]
    static extern uint GetDpiForWindow(IntPtr window);

    [DllImport("user32.dll")]
    static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    static Form source;
    static Label sourceLabel;

    [STAThread]
    static int Main(string[] args)
    {
        SetProcessDPIAware();
        var vibez = FindWindow(int.Parse(args[0]));
        var shots = args[1];
        Directory.CreateDirectory(shots);

        var ready = new ManualResetEvent(false);
        var ui = new Thread(() => RunSource(ready));
        ui.SetApartmentState(ApartmentState.STA);
        ui.Start();
        ready.WaitOne();

        var origin = new Point();
        ClientToScreen(vibez, ref origin);
        var scale = GetDpiForWindow(vibez) / 96.0 * double.Parse(args[2], System.Globalization.CultureInfo.InvariantCulture);
        Console.WriteLine("Vibez client area at {0},{1}, scale {2}", origin.X, origin.Y, scale);

        for (var step = 3; step + 3 < args.Length; step += 4)
        {
            var what = args[step];
            var target = new Point
            {
                X = origin.X + (int)(int.Parse(args[step + 1]) * scale),
                Y = origin.Y + (int)(int.Parse(args[step + 2]) * scale),
            };
            var name = args[step + 3];
            if (what == "click")
            {
                MoveTo(target);
                mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
                Thread.Sleep(80);
                mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
                Thread.Sleep(1500);
            }
            else
            {
                Drag(Path.GetFullPath(what), target, Path.Combine(shots, name + "-hover.png"));
                Thread.Sleep(2500);
            }
            Screenshot(Path.Combine(shots, name + ".png"));
        }

        source.Invoke((Action)(() => source.Close()));
        ui.Join();
        return 0;
    }

    static IntPtr FindWindow(int pid)
    {
        var process = Process.GetProcessById(pid);
        for (var tries = 0; tries < 120; tries++)
        {
            process.Refresh();
            if (process.MainWindowHandle != IntPtr.Zero)
            {
                return process.MainWindowHandle;
            }
            Thread.Sleep(500);
        }
        throw new TimeoutException("Vibez showed no window");
    }

    static void RunSource(ManualResetEvent ready)
    {
        source = new Form
        {
            Text = "Drag source",
            TopMost = true,
            FormBorderStyle = FormBorderStyle.FixedToolWindow,
            StartPosition = FormStartPosition.Manual,
            Size = new Size(220, 120),
        };
        var screen = Screen.PrimaryScreen.WorkingArea;
        source.Location = new System.Drawing.Point(screen.Right - 240, screen.Bottom - 140);
        sourceLabel = new Label { Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleCenter };
        source.Controls.Add(sourceLabel);
        source.Shown += (sender, e) => ready.Set();
        Application.Run(source);
    }

    static void Drag(string file, Point target, string hoverShot)
    {
        var dropped = new ManualResetEvent(false);
        var start = new Point();
        source.Invoke((Action)(() =>
        {
            sourceLabel.Text = Path.GetFileName(file);
            var centre = sourceLabel.PointToScreen(new System.Drawing.Point(sourceLabel.Width / 2, sourceLabel.Height / 2));
            start = new Point { X = centre.X, Y = centre.Y };
        }));

        MoveTo(start);
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        Thread.Sleep(200);
        // DoDragDrop runs a modal loop on the source's thread until the
        // button comes up, so the moves below come from this thread.
        source.BeginInvoke((Action)(() =>
        {
            var data = new DataObject(DataFormats.FileDrop, new[] { file });
            sourceLabel.DoDragDrop(data, DragDropEffects.Copy);
            dropped.Set();
        }));
        Thread.Sleep(400);

        const int steps = 40;
        for (var i = 1; i <= steps; i++)
        {
            MoveTo(new Point
            {
                X = start.X + (target.X - start.X) * i / steps,
                Y = start.Y + (target.Y - start.Y) * i / steps,
            });
            Thread.Sleep(25);
        }
        // A last nudge, so the drop target's DragOver runs at the target.
        MoveTo(new Point { X = target.X + 1, Y = target.Y });
        Thread.Sleep(150);
        MoveTo(target);
        Thread.Sleep(1500);
        Screenshot(hoverShot);

        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        // Straight back off Vibez, so no ordinary mouse move reaches it after
        // the drop. A drop then lands only where the poll found a target.
        MoveTo(start);
        if (!dropped.WaitOne(10000))
        {
            throw new TimeoutException("the drag did not end");
        }
    }

    static void MoveTo(Point point)
    {
        var screen = Screen.PrimaryScreen.Bounds;
        var x = point.X * 65535 / (screen.Width - 1);
        var y = point.Y * 65535 / (screen.Height - 1);
        mouse_event(Move | Absolute, x, y, 0, UIntPtr.Zero);
    }

    static void Screenshot(string path)
    {
        var screen = Screen.PrimaryScreen.Bounds;
        using (var image = new Bitmap(screen.Width, screen.Height))
        {
            using (var graphics = Graphics.FromImage(image))
            {
                graphics.CopyFromScreen(screen.Location, System.Drawing.Point.Empty, screen.Size);
            }
            image.Save(path, ImageFormat.Png);
        }
        Console.WriteLine("Saved {0}", path);
    }
}
