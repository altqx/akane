'use client'

import { useState, useEffect, useCallback } from 'react'
import Navbar from '@/components/Navbar'
import Button from '@/components/Button'
import Input from '@/components/Input'
import VideoPreviewModal from '@/components/VideoPreviewModal'
import VideoEditModal from '@/components/VideoEditModal'

interface Video {
  id: string
  name: string
  tags: string[]
  available_resolutions: string[]
  duration: number
  created_at: string
  playlist_url: string | null
  player_url: string | null
  thumbnail_url: string | null
  view_count: number
}

interface VideoResponse {
  items: Video[]
  page: number
  page_size: number
  total: number
  has_next: boolean
  has_prev: boolean
}

export default function Videos() {
  const [videos, setVideos] = useState<Video[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Filters
  const [nameFilter, setNameFilter] = useState('')
  const [tagFilter, setTagFilter] = useState('')
  const [pageSize, setPageSize] = useState(20)
  const [page, setPage] = useState(1)

  // Pagination state from response
  const [total, setTotal] = useState(0)
  const [hasNext, setHasNext] = useState(false)
  const [hasPrev, setHasPrev] = useState(false)

  const [copiedId, setCopiedId] = useState<string | null>(null)

  // Modal state
  const [previewVideo, setPreviewVideo] = useState<Video | null>(null)
  const [editVideo, setEditVideo] = useState<Video | null>(null)

  // Selection state
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [isDeleting, setIsDeleting] = useState(false)
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false)

  const loadVideos = useCallback(async () => {
    setLoading(true)
    setError(null)

    const params = new URLSearchParams()
    params.set('page', page.toString())
    params.set('page_size', pageSize.toString())
    if (nameFilter) params.set('name', nameFilter)
    if (tagFilter) params.set('tag', tagFilter)

    try {
      const token = localStorage.getItem('admin_token')
      const res = await fetch(`/api/videos?${params.toString()}`, {
        headers: {
          Authorization: `Bearer ${token}`
        }
      })
      if (!res.ok) {
        const text = await res.text()
        throw new Error(text || 'Request failed')
      }
      const data: VideoResponse = await res.json()

      setVideos(data.items || [])
      setTotal(data.total || 0)
      setHasNext(data.has_next)
      setHasPrev(data.has_prev)
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : String(err)
      setError(errorMessage)
      setVideos([])
    } finally {
      setLoading(false)
    }
  }, [page, pageSize, nameFilter, tagFilter])

  // Initial load
  useEffect(() => {
    loadVideos()
  }, [loadVideos])

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault()
    setPage(1) // Reset to page 1 on new search
    loadVideos()
  }

  const formatDuration = (seconds: number) => {
    const s = Number(seconds) || 0
    const h = Math.floor(s / 3600)
    const m = Math.floor((s % 3600) / 60)
    const sec = s % 60
    if (h > 0) {
      return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`
    }
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`
  }

  const copyEmbedCode = async (video: Video) => {
    if (!video.player_url) return
    const code = `<iframe src="${window.location.origin}${video.player_url}" width="100%" height="100%" frameborder="0" allowfullscreen></iframe>`
    try {
      await navigator.clipboard.writeText(code)
      setCopiedId(video.id)
      setTimeout(() => setCopiedId(null), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  const copyLink = async (video: Video) => {
    if (!video.player_url) return
    const code = `${window.location.origin}${video.player_url}`
    try {
      await navigator.clipboard.writeText(code)
      setCopiedId(video.id)
      setTimeout(() => setCopiedId(null), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  const handleSaveVideo = async (id: string, name: string, tags: string[]) => {
    const token = localStorage.getItem('admin_token')
    const res = await fetch(`/api/videos/${id}`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`
      },
      body: JSON.stringify({ name, tags })
    })

    if (!res.ok) {
      const text = await res.text()
      throw new Error(text || 'Failed to update video')
    }

    // Update local state
    setVideos((prev) =>
      prev.map((v) => (v.id === id ? { ...v, name, tags } : v))
    )
  }

  // Selection handlers
  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const toggleSelectAll = () => {
    if (selectedIds.size === videos.length) {
      setSelectedIds(new Set())
    } else {
      setSelectedIds(new Set(videos.map((v) => v.id)))
    }
  }

  const handleDeleteSelected = async () => {
    if (selectedIds.size === 0) return

    setIsDeleting(true)
    setError(null)

    try {
      const token = localStorage.getItem('admin_token')
      const res = await fetch('/api/videos', {
        method: 'DELETE',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`
        },
        body: JSON.stringify({ ids: Array.from(selectedIds) })
      })

      if (!res.ok) {
        const text = await res.text()
        throw new Error(text || 'Failed to delete videos')
      }

      // Clear selection and reload
      setSelectedIds(new Set())
      setDeleteConfirmOpen(false)
      loadVideos()
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : String(err)
      setError(errorMessage)
    } finally {
      setIsDeleting(false)
    }
  }

  return (
    <div className='min-h-screen bg-base-200 p-10 font-sans'>
      <div className='mx-auto max-w-7xl'>
        <div className='flex justify-between items-center mb-8'>
          <div>
            <h1 className='text-3xl font-bold tracking-tight'>Videos</h1>
            <p className='text-base-content/70 mt-1'>Manage and organize your video library.</p>
          </div>
          <Navbar />
        </div>

        <form
          onSubmit={handleSearch}
          className='mb-8 flex flex-wrap items-end gap-4 rounded-xl bg-base-100 p-4 shadow-sm'
        >
          <div className='w-full sm:w-auto flex-1 min-w-[200px]'>
            <Input
              label='Name contains'
              placeholder='Search videos...'
              value={nameFilter}
              onChange={(e) => setNameFilter(e.target.value)}
            />
          </div>
          <div className='w-full sm:w-auto flex-1 min-w-[200px]'>
            <Input
              label='Tag contains'
              placeholder='Filter by tag...'
              value={tagFilter}
              onChange={(e) => setTagFilter(e.target.value)}
            />
          </div>
          <div className='form-control'>
            <label htmlFor='pageSize' className='label'>
              <span className='label-text'>Page size</span>
            </label>
            <select
              id='pageSize'
              value={pageSize}
              onChange={(e) => {
                setPageSize(Number(e.target.value))
                setPage(1)
              }}
              className='select select-bordered w-full max-w-xs'
            >
              <option value='10'>10</option>
              <option value='20'>20</option>
              <option value='50'>50</option>
            </select>
          </div>
          <div className='pb-1 flex gap-2'>
            <Button type='submit' disabled={loading}>
              Search
            </Button>
            {selectedIds.size > 0 && (
              <Button
                type='button'
                variant='outline'
                onClick={() => setDeleteConfirmOpen(true)}
                className='btn-error'
              >
                Delete ({selectedIds.size})
              </Button>
            )}
          </div>
        </form>

        {error && (
          <div role='alert' className='alert alert-error mb-6'>
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
            <span>{error}</span>
          </div>
        )}

        <div className='overflow-x-auto rounded-xl bg-base-100 shadow-sm'>
          <table className='table w-full'>
            <thead>
              <tr>
                <th className='w-10'>
                  <input
                    type='checkbox'
                    className='checkbox checkbox-sm'
                    checked={videos.length > 0 && selectedIds.size === videos.length}
                    onChange={toggleSelectAll}
                  />
                </th>
                <th>Video</th>
                <th>Tags</th>
                <th>Stats</th>
                <th>Duration</th>
                <th>Created</th>
                <th className='text-right'>Actions</th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <tr>
                  <td colSpan={7} className='text-center py-12'>
                    <span className='loading loading-spinner loading-lg'></span>
                    <div className='mt-2'>Loading videos...</div>
                  </td>
                </tr>
              ) : videos.length === 0 ? (
                <tr>
                  <td colSpan={7} className='text-center py-12 text-base-content/70'>
                    No videos found matching your criteria.
                  </td>
                </tr>
              ) : (
                videos.map((video, idx) => (
                  <tr key={idx} className={`hover ${selectedIds.has(video.id) ? 'bg-base-200' : ''}`}>
                    <td>
                      <input
                        type='checkbox'
                        className='checkbox checkbox-sm'
                        checked={selectedIds.has(video.id)}
                        onChange={() => toggleSelect(video.id)}
                      />
                    </td>
                    <td>
                      <div className='flex items-center gap-3'>
                        <div className='avatar'>
                          <div className='mask mask-squircle w-16 h-10'>
                            {video.thumbnail_url ? (
                              // eslint-disable-next-line @next/next/no-img-element
                              <img src={video.thumbnail_url} alt={video.name} />
                            ) : (
                              <div className='w-full h-full bg-base-300 flex items-center justify-center text-xs'>
                                No img
                              </div>
                            )}
                          </div>
                        </div>
                        <div className='font-bold max-w-[200px] truncate' title={video.name}>
                          {video.name}
                        </div>
                      </div>
                    </td>
                    <td>
                      <div className='flex flex-wrap gap-1'>
                        {video.tags.map((tag, i) => (
                          <span key={i} className='badge badge-secondary badge-outline badge-sm'>
                            {tag}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td>
                      <div className='flex flex-col text-xs'>
                        <span className='text-base-content/70'>
                          <span className='font-bold text-base-content'>{video.view_count.toLocaleString()}</span> views
                        </span>
                        <span className='text-base-content/50'>{video.available_resolutions.length} qualities</span>
                      </div>
                    </td>
                    <td className='tabular-nums text-base-content/70'>{formatDuration(video.duration)}</td>
                    <td className='text-base-content/70 text-xs'>{new Date(video.created_at).toLocaleDateString()}</td>
                    <td className='text-right'>
                      <div className='flex justify-end gap-2'>
                        {video.player_url && (
                          <Button
                            size='sm'
                            variant='ghost'
                            onClick={() => setPreviewVideo(video)}
                            className='btn-xs'
                            title='Preview video'
                          >
                            <svg xmlns='http://www.w3.org/2000/svg' className='h-4 w-4' fill='none' viewBox='0 0 24 24' stroke='currentColor'>
                              <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z' />
                              <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M21 12a9 9 0 11-18 0 9 9 0 0118 0z' />
                            </svg>
                          </Button>
                        )}
                        <Button
                          size='sm'
                          variant='ghost'
                          onClick={() => setEditVideo(video)}
                          className='btn-xs'
                          title='Edit video'
                        >
                          <svg xmlns='http://www.w3.org/2000/svg' className='h-4 w-4' fill='none' viewBox='0 0 24 24' stroke='currentColor'>
                            <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z' />
                          </svg>
                        </Button>
                        {video.player_url && (
                          <Button size='sm' variant='secondary' onClick={() => copyEmbedCode(video)} className='btn-xs'>
                            {copiedId === video.id ? 'Copied!' : 'Copy Embed'}
                          </Button>
                        )}
                        {video.player_url && (
                          <Button size='sm' variant='secondary' onClick={() => copyLink(video)} className='btn-xs'>
                            {copiedId === video.id ? 'Copied!' : 'Copy Link'}
                          </Button>
                        )}
                        {video.playlist_url && (
                          <Button
                            size='sm'
                            variant='outline'
                            onClick={() => window.open(video.playlist_url!, '_blank')}
                            className='btn-xs'
                          >
                            Open
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <div className='mt-4 flex items-center justify-between'>
          <div className='join'>
            <button
              className='join-item btn btn-sm'
              disabled={!hasPrev || loading}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
            >
              Previous
            </button>
            <button
              className='join-item btn btn-sm'
              disabled={!hasNext || loading}
              onClick={() => setPage((p) => p + 1)}
            >
              Next
            </button>
          </div>
          <div className='text-sm text-base-content/70'>
            {total > 0 ? (
              <>
                Showing {(page - 1) * pageSize + 1}–{Math.min(page * pageSize, total)} of {total} videos
              </>
            ) : (
              'No results'
            )}
          </div>
        </div>
      </div>

      {/* Video Preview Modal */}
      <VideoPreviewModal
        isOpen={!!previewVideo}
        onClose={() => setPreviewVideo(null)}
        playerUrl={previewVideo?.player_url || ''}
        videoName={previewVideo?.name || ''}
      />

      {/* Video Edit Modal */}
      <VideoEditModal
        isOpen={!!editVideo}
        onClose={() => setEditVideo(null)}
        video={editVideo}
        onSave={handleSaveVideo}
      />

      {/* Delete Confirmation Modal */}
      {deleteConfirmOpen && (
        <div className='modal modal-open'>
          <div className='modal-box'>
            <h3 className='font-bold text-lg text-error'>Confirm Deletion</h3>
            <p className='py-4'>
              Are you sure you want to delete {selectedIds.size} video{selectedIds.size !== 1 ? 's' : ''}? 
              This action cannot be undone.
            </p>
            <div className='modal-action'>
              <button
                className='btn'
                onClick={() => setDeleteConfirmOpen(false)}
                disabled={isDeleting}
              >
                Cancel
              </button>
              <button
                className='btn btn-error'
                onClick={handleDeleteSelected}
                disabled={isDeleting}
              >
                {isDeleting ? (
                  <>
                    <span className='loading loading-spinner loading-sm'></span>
                    Deleting...
                  </>
                ) : (
                  'Delete'
                )}
              </button>
            </div>
          </div>
          <div className='modal-backdrop' onClick={() => !isDeleting && setDeleteConfirmOpen(false)}></div>
        </div>
      )}
    </div>
  )
}
