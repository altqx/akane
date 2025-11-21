'use client';

import { useEffect, useState } from 'react';
import Navbar from '@/components/Navbar';

interface ViewHistoryItem {
  date: string;
  count: number;
}

export default function AnalyticsPage() {
  const [activeViewers, setActiveViewers] = useState<Record<string, number>>({});
  const [history, setHistory] = useState<ViewHistoryItem[]>([]);
  const [totalActive, setTotalActive] = useState(0);

  useEffect(() => {
    // Fetch history
    fetch('/api/analytics/history')
      .then((res) => res.json())
      .then((data) => setHistory(data))
      .catch((err) => console.error('Failed to fetch history:', err));

    // Connect to SSE for realtime updates
    const eventSource = new EventSource('/api/analytics/realtime');

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        setActiveViewers(data);
        const total = Object.values(data).reduce((acc: number, curr: unknown) => acc + (curr as number), 0);
        setTotalActive(total);
      } catch (e) {
        console.error('Failed to parse SSE data:', e);
      }
    };

    return () => {
      eventSource.close();
    };
  }, []);

  return (
    <div className="min-h-screen bg-background p-10 font-sans text-foreground">
      <div className="mx-auto max-w-6xl">
        <h1 className="mb-2 text-3xl font-bold">Realtime Analytics</h1>
        <p className="mb-6 text-muted-foreground">Monitor active viewers and historical view trends.</p>
        
        <Navbar />

        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {/* Active Viewers Card */}
          <div className="rounded-lg border border-border bg-card p-6 text-card-foreground shadow-sm">
            <h2 className="text-xl font-semibold mb-4">Active Viewers Now</h2>
            <div className="text-6xl font-bold text-primary">{totalActive}</div>
            <div className="mt-6">
              <h3 className="text-lg font-medium mb-3 text-muted-foreground">By Video</h3>
              <ul className="space-y-2">
                {Object.entries(activeViewers).map(([videoId, count]) => (
                  <li key={videoId} className="flex justify-between items-center bg-muted/50 p-3 rounded-md border border-border">
                    <span className="truncate max-w-[250px] text-sm font-medium">{videoId}</span>
                    <span className="font-bold text-primary">{count}</span>
                  </li>
                ))}
                {Object.keys(activeViewers).length === 0 && (
                  <li className="text-muted-foreground text-sm italic">No active viewers currently</li>
                )}
              </ul>
            </div>
          </div>

          {/* History Chart (Simple Bar Chart) */}
          <div className="rounded-lg border border-border bg-card p-6 text-card-foreground shadow-sm">
            <h2 className="text-xl font-semibold mb-4">Views Last 30 Days</h2>
            <div className="flex items-end space-x-2 h-64 overflow-x-auto pb-2">
              {history.map((item) => {
                const maxCount = Math.max(...history.map((h) => h.count), 1);
                const heightPercentage = (item.count / maxCount) * 100;
                return (
                  <div key={item.date} className="flex flex-col items-center space-y-2 min-w-[40px] group">
                    <div className="relative w-full h-full flex items-end">
                      <div
                        className="w-full bg-primary/80 hover:bg-primary rounded-t transition-all duration-300"
                        style={{ height: `${heightPercentage}%` }}
                      >
                        <div className="opacity-0 group-hover:opacity-100 absolute bottom-full left-1/2 -translate-x-1/2 mb-2 bg-popover text-popover-foreground text-xs py-1 px-2 rounded border border-border whitespace-nowrap z-10 pointer-events-none">
                          {item.count} views
                        </div>
                      </div>
                    </div>
                    <span className="text-[10px] text-muted-foreground rotate-45 origin-left mt-2 whitespace-nowrap">
                      {new Date(item.date).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}
                    </span>
                  </div>
                );
              })}
              {history.length === 0 && (
                <div className="flex items-center justify-center w-full h-full text-muted-foreground">
                  No history data available
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
