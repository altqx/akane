'use client'

import { useState, useEffect, useCallback, MouseEvent } from 'react'

interface QueueItem {
  upload_id: string
  stage: string
  current_chunk: number
  total_chunks: number
  percentage: number
  details: string | null
  status: string
  video_name: string | null
  created_at: number // Unix timestamp in milliseconds for queue ordering
}

interface QueueListResponse {
  items: QueueItem[]
  active_count: number
  completed_count: number
  failed_count: number
}

function formatSince(timestampMs: number | null | undefined) {
  if (!timestampMs) return '—'
  const deltaSec = Math.max(0, Math.floor((Date.now() - timestampMs) / 1000))
  if (deltaSec < 60) return `${deltaSec}s ago`
  if (deltaSec < 3600) return `${Math.floor(deltaSec / 60)}m ago`
  const hours = Math.floor(deltaSec / 3600)
  const minutes = Math.floor((deltaSec % 3600) / 60)
  return `${hours}h ${minutes}m ago`
}

export default function ProcessingQueues() {
  const [queues, setQueues] = useState<QueueListResponse | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isCollapsed, setIsCollapsed] = useState(false)
  const [cancellingIds, setCancellingIds] = useState<Set<string>>(new Set())
  const [removingIds, setRemovingIds] = useState<Set<string>>(new Set())
  const [isClearingFailed, setIsClearingFailed] = useState(false)
  const [lastUpdatedAt, setLastUpdatedAt] = useState<number | null>(null)
  const [isCleaning, setIsCleaning] = useState(false)
  const [cleanupMessage, setCleanupMessage] = useState<string | null>(null)
  const [cleanupError, setCleanupError] = useState<string | null>(null)

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
      setLastUpdatedAt(Date.now())
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

  const isCancellable = (item: QueueItem) => {
    const cancellableStages = ['Initializing upload', 'Queued for processing', 'Receiving chunks']
    return (
      item.status === 'initializing' ||
      (item.status === 'processing' && cancellableStages.includes(item.stage))
    )
  }

  const handleCancel = async (uploadId: string) => {
    setCancellingIds((prev) => new Set(prev).add(uploadId))

    try {
      const token = localStorage.getItem('admin_token')
      const response = await fetch(`/api/queues/${uploadId}`, {
        method: 'DELETE',
        headers: {
          Authorization: `Bearer ${token}`
        }
      })

      if (!response.ok) {
        const text = await response.text()
        throw new Error(text || 'Failed to cancel')
      }

      // Refresh queues
      fetchQueues()
    } catch (err) {
      console.error('Failed to cancel queue item:', err)
    } finally {
      setCancellingIds((prev) => {
        const next = new Set(prev)
        next.delete(uploadId)
        return next
      })
    }
  }

  const handleRemove = async (uploadId: string) => {
    setRemovingIds((prev) => new Set(prev).add(uploadId))

    try {
      const token = localStorage.getItem('admin_token')
      const response = await fetch(`/api/queues/${uploadId}/remove`, {
        method: 'DELETE',
        headers: {
          Authorization: `Bearer ${token}`
        }
      })

      if (!response.ok) {
        const text = await response.text()
        throw new Error(text || 'Failed to remove')
      }

      // Refresh queues
      fetchQueues()
    } catch (err) {
      console.error('Failed to remove queue item:', err)
    } finally {
      setRemovingIds((prev) => {
        const next = new Set(prev)
        next.delete(uploadId)
        return next
      })
    }
  }

  const handleClearAllFailed = async (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation()
    setIsClearingFailed(true)

    try {
      const token = localStorage.getItem('admin_token')
      const response = await fetch('/api/queues/failed', {
        method: 'DELETE',
        headers: {
          Authorization: `Bearer ${token}`
        }
      })

      if (!response.ok) {
        const text = await response.text()
        throw new Error(text || 'Failed to clear failed items')
      }

      fetchQueues()
    } catch (err) {
      console.error('Failed to clear failed items:', err)
    } finally {
      setIsClearingFailed(false)
    }
  }

  const handleCleanup = async (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation()
    setIsCleaning(true)
    setCleanupMessage(null)
    setCleanupError(null)

    try {
      const token = localStorage.getItem('admin_token')
      const response = await fetch('/api/queues/cleanup', {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${token}`
        }
      })

      if (!response.ok) {
        const text = await response.text()
        throw new Error(text || 'Failed to run cleanup')
      }

      const data = await response.json()
      setCleanupMessage(data.message || 'Cleanup complete')
      fetchQueues()
    } catch (err) {
      setCleanupError(err instanceof Error ? err.message : 'Unknown error')
    } finally {
      setIsCleaning(false)
    }
  }

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
  const activeCount = queues?.active_count ?? 0
  const completedCount = queues?.completed_count ?? 0
  const failedCount = queues?.failed_count ?? 0

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

  return (
    <div className='card bg-base-100 shadow-xl mb-6'>
      <div className='card-body p-4'>
        <div className='flex items-center justify-between cursor-pointer' onClick={() => setIsCollapsed(!isCollapsed)}>
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
              {activeCount > 0 && <span className='text-primary'>{activeCount} processing</span>}
              {completedCount > 0 && <span className='text-success'>• {completedCount} completed</span>}
              {failedCount > 0 && <span className='text-error'>• {failedCount} failed</span>}
            </div>
            <button
              className='btn btn-outline btn-xs'
              onClick={handleCleanup}
              disabled={isCleaning}
              title='Clean up stale uploads'
            >
              {isCleaning ? (
                <span className='loading loading-spinner loading-xs'></span>
              ) : (
                'Cleanup'
              )}
            </button>
            <span className='text-[11px] text-base-content/60'>Updated {formatSince(lastUpdatedAt)}</span>
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
            {(cleanupMessage || cleanupError) && (
              <div className={`alert ${cleanupError ? 'alert-error' : 'alert-success'} py-2 px-3`}>
                <span className='text-sm'>{cleanupError || cleanupMessage}</span>
              </div>
            )}
            {(!queues || queues.items.length === 0) && (
              <div className='text-sm text-base-content/60'>No processing activity right now.</div>
            )}
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
                          <div className='flex flex-col'>
                            <span
                              className='font-medium text-sm truncate max-w-[250px]'
                              title={item.video_name || item.upload_id}
                            >
                              {item.video_name || `${item.upload_id.substring(0, 8)}...`}
                            </span>
                            <span className='text-[11px] text-base-content/60'>Started {formatSince(item.created_at)}</span>
                            {item.video_name && (
                              <span className='text-xs text-base-content/50'>
                                ID: {item.upload_id.substring(0, 8)}...
                              </span>
                            )}
                          </div>
                          {getStatusBadge(item.status)}
                        </div>
                        <div className='flex items-center gap-2'>
                          <div className='text-right'>
                            <div className='text-sm font-bold'>{item.percentage}%</div>
                            {item.total_chunks > 0 && (
                              <div className='text-[11px] text-base-content/60'>
                                {item.current_chunk}/{item.total_chunks} chunks
                              </div>
                            )}
                          </div>
                          {isCancellable(item) && (
                            <button
                              className='btn btn-ghost btn-xs text-error'
                              onClick={() => handleCancel(item.upload_id)}
                              disabled={cancellingIds.has(item.upload_id)}
                              title='Cancel this upload'
                            >
                              {cancellingIds.has(item.upload_id) ? (
                                <span className='loading loading-spinner loading-xs'></span>
                              ) : (
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
                                >
                                  <circle cx='12' cy='12' r='10' />
                                  <line x1='15' x2='9' y1='9' y2='15' />
                                  <line x1='9' x2='15' y1='9' y2='15' />
                                </svg>
                              )}
                            </button>
                          )}
                        </div>
                      </div>
                      <progress
                        className='progress progress-primary w-full h-2'
                        value={item.percentage}
                        max='100'
                      ></progress>
                      <div className='flex justify-between mt-1 text-xs text-base-content/70'>
                        <div className='flex flex-wrap items-center gap-2'>
                          <span className='badge badge-ghost badge-xs'>{item.stage}</span>
                          {item.details && (
                            <span className='truncate max-w-[200px]' title={item.details}>
                              {item.details}
                            </span>
                          )}
                        </div>
                        <span className='text-[11px] text-base-content/60'>ID {item.upload_id.substring(0, 8)}...</span>
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
                        <span className='text-sm truncate max-w-[250px]' title={item.video_name || item.upload_id}>
                          {item.video_name || `${item.upload_id.substring(0, 8)}...`}
                        </span>
                        <span className='text-[11px] text-base-content/60'>Queued {formatSince(item.created_at)}</span>
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
                <div className='flex items-center justify-between mb-2'>
                  <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider'>
                    Failed ({failedItems.length})
                  </div>
                  <button
                    className='btn btn-ghost btn-xs text-error hover:bg-error/20'
                    onClick={handleClearAllFailed}
                    disabled={isClearingFailed}
                    title='Clear all failed items'
                  >
                    {isClearingFailed ? (
                      <span className='loading loading-spinner loading-xs'></span>
                    ) : (
                      <>
                        <svg
                          xmlns='http://www.w3.org/2000/svg'
                          width='12'
                          height='12'
                          viewBox='0 0 24 24'
                          fill='none'
                          stroke='currentColor'
                          strokeWidth='2'
                          strokeLinecap='round'
                          strokeLinejoin='round'
                        >
                          <path d='M3 6h18' />
                          <path d='M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6' />
                          <path d='M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2' />
                        </svg>
                        Clear All
                      </>
                    )}
                  </button>
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
                        <span className='text-sm truncate max-w-[250px]' title={item.video_name || item.upload_id}>
                          {item.video_name || `${item.upload_id.substring(0, 8)}...`}
                        </span>
                        <span className='text-[11px] text-base-content/60'>Started {formatSince(item.created_at)}</span>
                      </div>
                      <div className='flex items-center gap-2'>
                        {item.details && (
                          <span className='text-xs text-error truncate max-w-[150px]' title={item.details}>
                            {item.details}
                          </span>
                        )}
                        {getStatusBadge(item.status)}
                        <button
                          className='btn btn-ghost btn-xs text-error hover:bg-error/20'
                          onClick={() => handleRemove(item.upload_id)}
                          disabled={removingIds.has(item.upload_id)}
                          title='Remove failed item'
                        >
                          {removingIds.has(item.upload_id) ? (
                            <span className='loading loading-spinner loading-xs'></span>
                          ) : (
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
                            >
                              <path d='M3 6h18' />
                              <path d='M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6' />
                              <path d='M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2' />
                            </svg>
                          )}
                        </button>
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
