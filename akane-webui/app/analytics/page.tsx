'use client'

import { useEffect, useState, useMemo } from 'react'
import Navbar from '@/components/Navbar'

interface ViewHistoryItem {
  date: string
  count: number
}

interface VideoSummary {
  id: string
  name: string
  view_count: number
  created_at: string
  thumbnail_url: string
}

export default function AnalyticsPage() {
  const [activeViewers, setActiveViewers] = useState<Record<string, number>>({})
  const [history, setHistory] = useState<ViewHistoryItem[]>([])
  const [videos, setVideos] = useState<VideoSummary[]>([])
  const [totalActive, setTotalActive] = useState(0)
  const [totalLifetime, setTotalLifetime] = useState(0)
  const [sortConfig, setSortConfig] = useState<{ key: 'active' | 'lifetime' | 'name'; direction: 'asc' | 'desc' }>({
    key: 'active',
    direction: 'desc'
  })

  useEffect(() => {
    // Fetch history
    fetch('/api/analytics/history')
      .then((res) => res.json())
      .then((data) => setHistory(data))
      .catch((err) => console.error('Failed to fetch history:', err))

    // Fetch videos summary
    fetch('/api/analytics/videos')
      .then((res) => res.json())
      .then((data: VideoSummary[]) => {
        setVideos(data)
        const total = data.reduce((acc, curr) => acc + curr.view_count, 0)
        setTotalLifetime(total)
      })
      .catch((err) => console.error('Failed to fetch videos:', err))

    // Connect to SSE for realtime updates
    const eventSource = new EventSource('/api/analytics/realtime')

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        setActiveViewers(data)
        const total = Object.values(data).reduce((acc: number, curr: unknown) => acc + (curr as number), 0)
        setTotalActive(total)
      } catch (e) {
        console.error('Failed to parse SSE data:', e)
      }
    }

    return () => {
      eventSource.close()
    }
  }, [])

  const sortedData = useMemo(() => {
    const data = videos.map((video) => ({
      ...video,
      active: activeViewers[video.id] || 0
    }))

    return data.sort((a, b) => {
      if (sortConfig.key === 'active') {
        return sortConfig.direction === 'asc' ? a.active - b.active : b.active - a.active
      } else if (sortConfig.key === 'lifetime') {
        return sortConfig.direction === 'asc' ? a.view_count - b.view_count : b.view_count - a.view_count
      } else {
        return sortConfig.direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name)
      }
    })
  }, [videos, activeViewers, sortConfig])

  const handleSort = (key: 'active' | 'lifetime' | 'name') => {
    setSortConfig((current) => ({
      key,
      direction: current.key === key && current.direction === 'desc' ? 'asc' : 'desc'
    }))
  }

  return (
    <div className='min-h-screen bg-base-200 p-8 font-sans'>
      <div className='mx-auto max-w-7xl'>
        <div className='flex justify-between items-center mb-8'>
          <div>
            <h1 className='text-3xl font-bold tracking-tight'>Analytics Dashboard</h1>
            <p className='text-base-content/70 mt-1'>Realtime audience insights and performance metrics.</p>
          </div>
          <Navbar />
        </div>

        {/* Top Metrics */}
        <div className='grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-12'>
          <div className='stats shadow'>
            <div className='stat'>
              <div className='stat-title'>Active Viewers</div>
              <div className='stat-value text-primary flex items-center gap-2'>
                {totalActive.toLocaleString()}
                <span className='relative flex h-3 w-3'>
                  <span className='animate-ping absolute inline-flex h-full w-full rounded-full bg-success opacity-75'></span>
                  <span className='relative inline-flex rounded-full h-3 w-3 bg-success'></span>
                </span>
              </div>
              <div className='stat-desc'>Watching right now</div>
            </div>
          </div>

          <div className='stats shadow'>
            <div className='stat'>
              <div className='stat-title'>Lifetime Views</div>
              <div className='stat-value text-primary'>{totalLifetime.toLocaleString()}</div>
              <div className='stat-desc'>Total across all videos</div>
            </div>
          </div>
        </div>

        {/* History Chart */}
        <div className='card bg-base-100 shadow-xl mb-12'>
          <div className='card-body'>
            <h2 className='card-title mb-6'>Views Last 30 Days</h2>
            <div className='h-64 w-full flex items-end space-x-1'>
              {history.map((item) => {
                const maxCount = Math.max(...history.map((h) => h.count), 1)
                const heightPercentage = (item.count / maxCount) * 100
                return (
                  <div key={item.date} className='flex-1 flex flex-col items-center group relative h-full justify-end'>
                    <div
                      className='w-full bg-primary/80 hover:bg-primary rounded-t-sm transition-all duration-300 ease-out relative'
                      style={{ height: `${heightPercentage}%`, minHeight: '4px' }}
                    >
                      <div className='opacity-0 group-hover:opacity-100 absolute bottom-full left-1/2 -translate-x-1/2 mb-2 bg-base-300 text-base-content text-xs py-1 px-2 rounded border border-base-200 whitespace-nowrap z-10 pointer-events-none shadow-md transition-opacity duration-200'>
                        <div className='font-bold'>{item.count} views</div>
                        <div className='text-[10px] text-base-content/70'>{item.date}</div>
                      </div>
                    </div>
                  </div>
                )
              })}
              {history.length === 0 && (
                <div className='w-full h-full flex items-center justify-center text-base-content/50 border border-dashed border-base-300 rounded-lg'>
                  No history data available
                </div>
              )}
            </div>
            <div className='flex justify-between mt-2 text-xs text-base-content/70'>
              <span>30 days ago</span>
              <span>Today</span>
            </div>
          </div>
        </div>

        {/* Realtime Data Table */}
        <div>
          <h2 className='text-xl font-semibold mb-6'>Video Performance</h2>
          <div className='overflow-x-auto rounded-xl bg-base-100 shadow-sm'>
            <table className='table w-full'>
              <thead>
                <tr>
                  <th
                    className='cursor-pointer hover:text-base-content transition-colors'
                    onClick={() => handleSort('name')}
                  >
                    Video {sortConfig.key === 'name' && (sortConfig.direction === 'asc' ? '↑' : '↓')}
                  </th>
                  <th
                    className='text-right cursor-pointer hover:text-base-content transition-colors'
                    onClick={() => handleSort('active')}
                  >
                    Active Viewers {sortConfig.key === 'active' && (sortConfig.direction === 'asc' ? '↑' : '↓')}
                  </th>
                  <th
                    className='text-right cursor-pointer hover:text-base-content transition-colors'
                    onClick={() => handleSort('lifetime')}
                  >
                    Lifetime Views {sortConfig.key === 'lifetime' && (sortConfig.direction === 'asc' ? '↑' : '↓')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {sortedData.map((video) => (
                  <tr key={video.id} className='hover'>
                    <td>
                      <div className='flex items-center space-x-4'>
                        <div className='avatar'>
                          <div className='mask mask-squircle w-16 h-10'>
                            {/* eslint-disable-next-line @next/next/no-img-element */}
                            <img src={video.thumbnail_url} alt='' />
                          </div>
                        </div>
                        <span className='font-medium truncate max-w-[300px]'>{video.name}</span>
                      </div>
                    </td>
                    <td className='text-right font-mono font-medium'>
                      {video.active > 0 ? (
                        <span className='inline-flex items-center text-success'>
                          <span className='w-2 h-2 bg-success rounded-full mr-2 animate-pulse'></span>
                          {video.active}
                        </span>
                      ) : (
                        <span className='text-base-content/50'>-</span>
                      )}
                    </td>
                    <td className='text-right font-mono text-base-content/70'>{video.view_count.toLocaleString()}</td>
                  </tr>
                ))}
                {sortedData.length === 0 && (
                  <tr>
                    <td colSpan={3} className='text-center py-8 text-base-content/70'>
                      No videos found.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  )
}
