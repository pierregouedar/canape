using System;
using Windows.Foundation;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Shapes;

namespace CanapeWidget
{
    /// <summary>Fixed-size ring of samples, oldest first; NaN marks a missing value.</summary>
    sealed class SampleHistory
    {
        private readonly double[] values;
        private int start;
        private int count;

        public SampleHistory(int capacity)
        {
            values = new double[capacity];
        }

        public int Capacity
        {
            get { return values.Length; }
        }

        public int Count
        {
            get { return count; }
        }

        public double this[int index]
        {
            get { return values[(start + index) % values.Length]; }
        }

        public void Add(double? value)
        {
            var sample = value.HasValue && !double.IsNaN(value.Value) && !double.IsInfinity(value.Value) ? value.Value : double.NaN;
            if (count < values.Length)
            {
                values[(start + count) % values.Length] = sample;
                count++;
            }
            else
            {
                values[start] = sample;
                start = (start + 1) % values.Length;
            }
        }

        public void Clear()
        {
            start = 0;
            count = 0;
        }

        /// <summary>Largest known value, or NaN when there is none.</summary>
        public double Max()
        {
            var max = double.NaN;
            for (var i = 0; i < count; i++)
            {
                var value = this[i];
                if (!double.IsNaN(value) && (double.IsNaN(max) || value > max))
                {
                    max = value;
                }
            }
            return max;
        }
    }

    /// <summary>
    /// A sparkline drawn into Paths declared in XAML (inside a Canvas): one
    /// sample per status push, newest on the right edge, a faint area under the
    /// line and an optional dashed reference series (the TDP limit). Gaps in
    /// the data break the line. Rebuilds a few small geometries per push, nothing
    /// runs between pushes.
    /// </summary>
    sealed class LiveGraph
    {
        private readonly FrameworkElement plot;
        private readonly Path line;
        private readonly Path area;
        private readonly Path reference;
        private readonly SampleHistory values;
        private readonly SampleHistory references;

        public LiveGraph(FrameworkElement plot, Path line, Path area, Path reference, int capacity)
        {
            this.plot = plot;
            this.line = line;
            this.area = area;
            this.reference = reference;
            values = new SampleHistory(capacity);
            references = new SampleHistory(capacity);
            plot.SizeChanged += (sender, e) => Render();
        }

        /// <summary>Fixed top of the scale (1 for a [0, 1] ratio); 0 scales to the data.</summary>
        public double FixedMax { get; set; }

        /// <summary>Smallest top of the scale when scaling to the data.</summary>
        public double MinimumScale { get; set; } = 1;

        public void Add(double? value, double? referenceValue)
        {
            values.Add(value);
            references.Add(referenceValue);
        }

        public void Clear()
        {
            values.Clear();
            references.Clear();
            Render();
        }

        public void Render()
        {
            var width = plot.ActualWidth;
            var height = plot.ActualHeight;
            if (width <= 0 || height <= 0)
            {
                return;
            }

            var max = FixedMax;
            if (max <= 0)
            {
                max = MinimumScale;
                var top = values.Max();
                if (!double.IsNaN(top))
                {
                    max = Math.Max(max, top);
                }
                if (reference != null)
                {
                    top = references.Max();
                    if (!double.IsNaN(top))
                    {
                        max = Math.Max(max, top);
                    }
                }
                // Headroom so the line never sits on the top edge.
                max *= 1.15;
            }

            line.Data = Build(values, max, width, height, false);
            if (area != null)
            {
                area.Data = Build(values, max, width, height, true);
            }
            if (reference != null)
            {
                reference.Data = Build(references, max, width, height, false);
            }
        }

        private static Geometry Build(SampleHistory history, double max, double width, double height, bool filled)
        {
            var geometry = new PathGeometry();
            var step = width / Math.Max(history.Capacity - 1, 1);
            // Fewer samples than slots: they fill the right-hand side.
            var offset = history.Capacity - history.Count;
            PathFigure figure = null;
            PolyLineSegment segment = null;
            var lastX = 0.0;
            for (var i = 0; i < history.Count; i++)
            {
                var value = history[i];
                if (double.IsNaN(value))
                {
                    Finish(geometry, figure, segment, lastX, height, filled);
                    figure = null;
                    continue;
                }
                var x = (offset + i) * step;
                var ratio = Math.Min(Math.Max(value / max, 0), 1);
                // Keep a 1 px margin so the stroke is not cut at the edges.
                var y = height - 1 - ratio * (height - 2);
                var point = new Point(x, y);
                if (figure == null)
                {
                    figure = new PathFigure
                    {
                        StartPoint = filled ? new Point(x, height) : point,
                        IsClosed = filled,
                        IsFilled = filled,
                    };
                    segment = new PolyLineSegment();
                    if (filled)
                    {
                        segment.Points.Add(point);
                    }
                }
                else
                {
                    segment.Points.Add(point);
                }
                lastX = x;
            }
            Finish(geometry, figure, segment, lastX, height, filled);
            return geometry;
        }

        private static void Finish(PathGeometry geometry, PathFigure figure, PolyLineSegment segment, double lastX, double height, bool filled)
        {
            if (figure == null || segment.Points.Count == 0)
            {
                // A single isolated sample draws nothing.
                return;
            }
            if (filled)
            {
                segment.Points.Add(new Point(lastX, height));
            }
            figure.Segments.Add(segment);
            geometry.Figures.Add(figure);
        }
    }
}
