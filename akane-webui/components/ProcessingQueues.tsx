'use client'

import { useState, useEffect, useCallback } from 'react'

interface QueueItem {
  upload_id: string
  stage: string
  current_chunk: number
  total_chunks: number
  percentage: number
  details: string | null
  status: string
}

interface QueueListResponse {
  items: QueueItem[]
  active_count: number
  completed_count: number
  failed_count: number
}

export default function ProcessingQueues() {
  const [queues, setQueues] = useState<QueueListResponse | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isCollapsed, setIsCollapsed] = useState(false)

  const fetchQueues = useCallback(async () => {
    try {
      const token = localStorage.getItem('admin_token')
      const response = await fetch('/api/queues', {
        headers: {
          Authorization: `Bearer ${token}`
        }
      })

      if (!response.ok) {
        throw new Error('Failed to fetch queues')
      }

      const data: QueueListResponse = await response.json()
      setQueues(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unknown error')
    } finally {
      setIsLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchQueues()
    const interval = setInterval(fetchQueues, 2000) // Poll every 2 seconds
    return () => clearInterval(interval)
  }, [fetchQueues])

  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'processing':
        return <span className='badge badge-primary badge-sm'>Processing</span>
      case 'initializing':
        return <span className='badge badge-info badge-sm'>Initializing</span>
      case 'completed':
        return <span className='badge badge-success badge-sm'>Completed</span>
      case 'failed':
        return <span className='badge badge-error badge-sm'>Failed</span>
      default:
        return <span className='badge badge-ghost badge-sm'>{status}</span>
    }
  }

  const activeItems = queues?.items.filter((i) => i.status === 'processing' || i.status === 'initializing') || []
  const completedItems = queues?.items.filter((i) => i.status === 'completed') || []
  const failedItems = queues?.items.filter((i) => i.status === 'failed') || []

  if (isLoading) {
    return (
      <div className='card bg-base-100 shadow-xl mb-6'>
        <div className='card-body'>
          <div className='flex items-center gap-2'>
            <span className='loading loading-spinner loading-sm'></span>
            <span>Loading processing queues...</span>
          </div>
        </div>
      </div>
    )
  }

  if (error) {
    return (
      <div className='alert alert-error mb-6'>
        <svg
          xmlns='http://www.w3.org/2000/svg'
          className='stroke-current shrink-0 h-6 w-6'
          fill='none'
          viewBox='0 0 24 24'
        >
          <path
            strokeLinecap='round'
            strokeLinejoin='round'
            strokeWidth='2'
            d='M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z'
          />
        </svg>
        <span>Failed to load processing queues: {error}</span>
      </div>
    )
  }

  if (!queues || queues.items.length === 0) {
    return null // Don't show anything if there are no queues
  }

  return (
    <div className='card bg-base-100 shadow-xl mb-6'>
      <div className='card-body p-4'>
        <div
          className='flex items-center justify-between cursor-pointer'
          onClick={() => setIsCollapsed(!isCollapsed)}
        >
          <h3 className='card-title text-base flex items-center gap-2'>
            <svg
              xmlns='http://www.w3.org/2000/svg'
              width='20'
              height='20'
              viewBox='0 0 24 24'
              fill='none'
              stroke='currentColor'
              strokeWidth='2'
              strokeLinecap='round'
              strokeLinejoin='round'
            >
              <path d='M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8' />
              <path d='M21 3v5h-5' />
            </svg>
            Processing Queues
            {activeItems.length > 0 && (
              <span className='badge badge-primary badge-sm'>{activeItems.length} active</span>
            )}
          </h3>
          <div className='flex items-center gap-2'>
            <div className='flex gap-1 text-xs'>
              {queues.active_count > 0 && (
                <span className='text-primary'>{queues.active_count} processing</span>
              )}
              {queues.completed_count > 0 && (
                <span className='text-success'>• {queues.completed_count} completed</span>
              )}
              {queues.failed_count > 0 && (
                <span className='text-error'>• {queues.failed_count} failed</span>
              )}
            </div>
            <svg
              xmlns='http://www.w3.org/2000/svg'
              width='16'
              height='16'
              viewBox='0 0 24 24'
              fill='none'
              stroke='currentColor'
              strokeWidth='2'
              strokeLinecap='round'
              strokeLinejoin='round'
              className={`transition-transform ${isCollapsed ? '' : 'rotate-180'}`}
            >
              <polyline points='6 9 12 15 18 9' />
            </svg>
          </div>
        </div>

        {!isCollapsed && (
          <div className='mt-4 space-y-3'>
            {/* Active Items */}
            {activeItems.length > 0 && (
              <div>
                <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider mb-2'>
                  Active ({activeItems.length})
                </div>
                <div className='space-y-2'>
                  {activeItems.map((item) => (
                    <div key={item.upload_id} className='bg-base-200 rounded-lg p-3'>
                      <div className='flex items-center justify-between mb-2'>
                        <div className='flex items-center gap-2'>
                          <span className='loading loading-spinner loading-xs'></span>
                          <span className='font-medium text-sm truncate max-w-[200px]' title={item.upload_id}>
                            {item.upload_id.substring(0, 8)}...
                          </span>
                          {getStatusBadge(item.status)}
                        </div>
                        <span className='text-sm font-bold'>{item.percentage}%</span>
                      </div>
                      <progress
                        className='progress progress-primary w-full h-2'
                        value={item.percentage}
                        max='100'
                      ></progress>
                      <div className='flex justify-between mt-1 text-xs text-base-content/70'>
                        <span>{item.stage}</span>
                        <span>
                          {item.current_chunk > 0 ? `${item.current_chunk}/${item.total_chunks}` : ''}
                          {item.details && ` • ${item.details}`}
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Completed Items */}
            {completedItems.length > 0 && (
              <div>
                <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider mb-2'>
                  Recently Completed ({completedItems.length})
                </div>
                <div className='space-y-1'>
                  {completedItems.slice(0, 5).map((item) => (
                    <div
                      key={item.upload_id}
                      className='flex items-center justify-between bg-success/10 rounded-lg px-3 py-2'
                    >
                      <div className='flex items-center gap-2'>
                        <svg
                          xmlns='http://www.w3.org/2000/svg'
                          width='14'
                          height='14'
                          viewBox='0 0 24 24'
                          fill='none'
                          stroke='currentColor'
                          strokeWidth='2'
                          strokeLinecap='round'
                          strokeLinejoin='round'
                          className='text-success'
                        >
                          <polyline points='20 6 9 17 4 12' />
                        </svg>
                        <span className='text-sm truncate max-w-[200px]' title={item.upload_id}>
                          {item.upload_id.substring(0, 8)}...
                        </span>
                      </div>
                      {getStatusBadge(item.status)}
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Failed Items */}
            {failedItems.length > 0 && (
              <div>
                <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider mb-2'>
                  Failed ({failedItems.length})
                </div>
                <div className='space-y-1'>
                  {failedItems.slice(0, 5).map((item) => (
                    <div
                      key={item.upload_id}
                      className='flex items-center justify-between bg-error/10 rounded-lg px-3 py-2'
                    >
                      <div className='flex items-center gap-2'>
                        <svg
                          xmlns='http://www.w3.org/2000/svg'
                          width='14'
                          height='14'
                          viewBox='0 0 24 24'
                          fill='none'
                          stroke='currentColor'
                          strokeWidth='2'
                          strokeLinecap='round'
                          strokeLinejoin='round'
                          className='text-error'
                        >
                          <circle cx='12' cy='12' r='10' />
                          <line x1='15' x2='9' y1='9' y2='15' />
                          <line x1='9' x2='15' y1='9' y2='15' />
                        </svg>
                        <span className='text-sm truncate max-w-[200px]' title={item.upload_id}>
                          {item.upload_id.substring(0, 8)}...
                        </span>
                      </div>
                      <div className='flex items-center gap-2'>
                        {item.details && (
                          <span className='text-xs text-error truncate max-w-[150px]' title={item.details}>
                            {item.details}
                          </span>
                        )}
                        {getStatusBadge(item.status)}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}