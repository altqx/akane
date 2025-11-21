'use client'

import { useState, useEffect, useCallback } from 'react'
import Navbar from '@/components/Navbar'
import Button from '@/components/Button'
import Input from '@/components/Input'

interface Video {
  name: string
  tags: string[]
  available_resolutions: string[]
  duration: number
  created_at: string
  playlist_url: string | null
  thumbnail_url: string | null
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

  return (
    <div className='min-h-screen bg-background p-10 font-sans text-foreground'>
      <div className='mx-auto max-w-6xl'>
        <h1 className='mb-2 text-3xl font-bold'>Akane Videos Admin</h1>
        <p className='mb-6 text-muted-foreground'>Browse videos stored in videos.db, with search and pagination.</p>

        <Navbar />

        <form
          onSubmit={handleSearch}
          className='mb-8 flex flex-wrap items-end gap-4 rounded-lg border border-border bg-card p-4 text-card-foreground shadow-sm'
        >
          <div className='w-full sm:w-auto'>
            <Input
              label='Name contains'
              placeholder='Video name...'
              value={nameFilter}
              onChange={(e) => setNameFilter(e.target.value)}
            />
          </div>
          <div className='w-full sm:w-auto'>
            <Input
              label='Tag contains'
              placeholder='e.g. gaming'
              value={tagFilter}
              onChange={(e) => setTagFilter(e.target.value)}
            />
          </div>
          <div className='flex flex-col gap-2'>
            <label htmlFor='pageSize' className='text-sm font-medium leading-none text-foreground'>
              Page size
            </label>
            <select
              id='pageSize'
              value={pageSize}
              onChange={(e) => {
                setPageSize(Number(e.target.value))
                setPage(1)
              }}
              className='flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50'
            >
              <option value='10' className='bg-popover text-popover-foreground'>
                10
              </option>
              <option value='20' className='bg-popover text-popover-foreground'>
                20
              </option>
              <option value='50' className='bg-popover text-popover-foreground'>
                50
              </option>
            </select>
          </div>
          <Button type='submit' disabled={loading}>
            Search
          </Button>
        </form>

        {error && <div className='mb-6 rounded-md bg-destructive/15 p-4 text-destructive'>Error: {error}</div>}

        <div className='overflow-hidden rounded-lg border border-border bg-card text-card-foreground shadow-sm'>
          <table className='w-full text-left text-sm'>
            <thead className='bg-muted/50 text-muted-foreground'>
              <tr>
                <th className='border-b border-border px-4 py-3 font-medium'>Name</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Tags</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Resolutions</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Duration</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Created at</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Playlist</th>
                <th className='border-b border-border px-4 py-3 font-medium'>Thumbnail</th>
              </tr>
            </thead>
            <tbody className='divide-y divide-border'>
              {loading ? (
                <tr>
                  <td colSpan={7} className='px-4 py-8 text-center text-muted-foreground'>
                    Loading...
                  </td>
                </tr>
              ) : videos.length === 0 ? (
                <tr>
                  <td colSpan={7} className='px-4 py-8 text-center text-muted-foreground'>
                    No videos found.
                  </td>
                </tr>
              ) : (
                videos.map((video, idx) => (
                  <tr key={idx} className='hover:bg-muted/50 transition-colors'>
                    <td className='px-4 py-3 font-medium'>{video.name}</td>
                    <td className='px-4 py-3'>
                      <div className='flex flex-wrap gap-1'>
                        {video.tags.map((tag, i) => (
                          <span
                            key={i}
                            className='inline-flex items-center rounded-full border border-transparent bg-secondary px-2 py-0.5 text-xs font-semibold text-secondary-foreground transition-colors hover:bg-secondary/80'
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td className='px-4 py-3 text-muted-foreground'>{video.available_resolutions.join(', ')}</td>
                    <td className='px-4 py-3 tabular-nums text-muted-foreground'>{formatDuration(video.duration)}</td>
                    <td className='px-4 py-3 text-muted-foreground'>{video.created_at}</td>
                    <td className='px-4 py-3'>
                      {video.playlist_url ? (
                        <a
                          href={video.playlist_url}
                          target='_blank'
                          rel='noopener noreferrer'
                          className='text-primary hover:underline'
                        >
                          Open
                        </a>
                      ) : (
                        <span className='text-muted-foreground'>N/A</span>
                      )}
                    </td>
                    <td className='px-4 py-3'>
                      {video.thumbnail_url ? (
                        <a href={video.thumbnail_url} target='_blank' rel='noopener noreferrer'>
                          {/* eslint-disable-next-line @next/next/no-img-element */}
                          <img
                            src={video.thumbnail_url}
                            alt='Thumbnail'
                            className='h-12 w-20 rounded border border-border object-cover'
                          />
                        </a>
                      ) : (
                        <span className='text-xs text-muted-foreground'>No thumbnail</span>
                      )}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <div className='mt-4 flex items-center justify-between'>
          <div className='flex gap-2'>
            <Button
              variant='outline'
              size='sm'
              disabled={!hasPrev || loading}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
            >
              Prev
            </Button>
            <Button variant='outline' size='sm' disabled={!hasNext || loading} onClick={() => setPage((p) => p + 1)}>
              Next
            </Button>
          </div>
          <div className='text-sm text-muted-foreground'>
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
    </div>
  )
}
