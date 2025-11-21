'use client';

import { useState, useEffect, useCallback } from 'react';
import Navbar from '@/components/Navbar';
import Button from '@/components/Button';
import Input from '@/components/Input';

interface Video {
  name: string;
  tags: string[];
  available_resolutions: string[];
  duration: number;
  created_at: string;
  playlist_url: string | null;
  thumbnail_url: string | null;
}

interface VideoResponse {
  items: Video[];
  page: number;
  page_size: number;
  total: number;
  has_next: boolean;
  has_prev: boolean;
}

export default function Videos() {
  const [videos, setVideos] = useState<Video[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  // Filters
  const [nameFilter, setNameFilter] = useState('');
  const [tagFilter, setTagFilter] = useState('');
  const [pageSize, setPageSize] = useState(20);
  const [page, setPage] = useState(1);
  
  // Pagination state from response
  const [total, setTotal] = useState(0);
  const [hasNext, setHasNext] = useState(false);
  const [hasPrev, setHasPrev] = useState(false);

  const loadVideos = useCallback(async () => {
    setLoading(true);
    setError(null);

    const params = new URLSearchParams();
    params.set('page', page.toString());
    params.set('page_size', pageSize.toString());
    if (nameFilter) params.set('name', nameFilter);
    if (tagFilter) params.set('tag', tagFilter);

    try {
      const res = await fetch(`/api/videos?${params.toString()}`);
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || 'Request failed');
      }
      const data: VideoResponse = await res.json();
      
      setVideos(data.items || []);
      setTotal(data.total || 0);
      setHasNext(data.has_next);
      setHasPrev(data.has_prev);
      // Update page/pageSize from server response if needed, but usually local state is source of truth for request
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      setVideos([]);
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, nameFilter, tagFilter]);

  // Initial load
  useEffect(() => {
    loadVideos();
  }, [loadVideos]);

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    setPage(1); // Reset to page 1 on new search
    loadVideos();
  };

  const formatDuration = (seconds: number) => {
    const s = Number(seconds) || 0;
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = s % 60;
    if (h > 0) {
      return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
    }
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  };

  return (
    <div className="min-h-screen bg-white p-10 font-sans text-gray-900">
      <div className="mx-auto max-w-6xl">
        <h1 className="mb-2 text-3xl font-bold">Akane Videos Admin</h1>
        <p className="mb-6 text-gray-500">Browse videos stored in videos.db, with search and pagination.</p>
        
        <Navbar />

        <form onSubmit={handleSearch} className="mb-8 flex flex-wrap items-end gap-4 rounded bg-gray-50 p-4">
          <div className="w-full sm:w-auto">
            <Input 
              label="Name contains" 
              placeholder="Video name..." 
              value={nameFilter}
              onChange={(e) => setNameFilter(e.target.value)}
            />
          </div>
          <div className="w-full sm:w-auto">
            <Input 
              label="Tag contains" 
              placeholder="e.g. gaming" 
              value={tagFilter}
              onChange={(e) => setTagFilter(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor="pageSize" className="text-sm font-medium text-gray-700">Page size</label>
            <select 
              id="pageSize"
              value={pageSize}
              onChange={(e) => {
                setPageSize(Number(e.target.value));
                setPage(1);
              }}
              className="rounded border border-gray-300 px-3 py-2 text-sm focus:border-blue-600 focus:outline-none focus:ring-1 focus:ring-blue-600"
            >
              <option value="10">10</option>
              <option value="20">20</option>
              <option value="50">50</option>
            </select>
          </div>
          <Button type="submit" disabled={loading}>Search</Button>
        </form>

        {error && (
          <div className="mb-6 rounded bg-red-50 p-4 text-red-700">
            Error: {error}
          </div>
        )}

        <div className="overflow-hidden rounded border border-gray-200">
          <table className="w-full text-left text-sm">
            <thead className="bg-gray-50 text-gray-700">
              <tr>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Name</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Tags</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Resolutions</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Duration</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Created at</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Playlist</th>
                <th className="border-b border-gray-200 px-4 py-3 font-semibold">Thumbnail</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {loading ? (
                <tr>
                  <td colSpan={7} className="px-4 py-8 text-center text-gray-500">Loading...</td>
                </tr>
              ) : videos.length === 0 ? (
                <tr>
                  <td colSpan={7} className="px-4 py-8 text-center text-gray-500">No videos found.</td>
                </tr>
              ) : (
                videos.map((video, idx) => (
                  <tr key={idx} className="hover:bg-gray-50">
                    <td className="px-4 py-3 font-medium text-gray-900">{video.name}</td>
                    <td className="px-4 py-3">
                      <div className="flex flex-wrap gap-1">
                        {video.tags.map((tag, i) => (
                          <span key={i} className="rounded-full bg-gray-200 px-2 py-0.5 text-xs text-gray-700">
                            {tag}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {video.available_resolutions.join(', ')}
                    </td>
                    <td className="px-4 py-3 tabular-nums text-gray-600">
                      {formatDuration(video.duration)}
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {video.created_at}
                    </td>
                    <td className="px-4 py-3">
                      {video.playlist_url ? (
                        <a 
                          href={video.playlist_url} 
                          target="_blank" 
                          rel="noopener noreferrer"
                          className="text-blue-600 hover:underline"
                        >
                          Open
                        </a>
                      ) : (
                        <span className="text-gray-400">N/A</span>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      {video.thumbnail_url ? (
                        <a href={video.thumbnail_url} target="_blank" rel="noopener noreferrer">
                          {/* eslint-disable-next-line @next/next/no-img-element */}
                          <img 
                            src={video.thumbnail_url} 
                            alt="Thumbnail" 
                            className="h-12 w-20 rounded border border-gray-200 object-cover"
                          />
                        </a>
                      ) : (
                        <span className="text-xs text-gray-400">No thumbnail</span>
                      )}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <div className="mt-4 flex items-center justify-between">
          <div className="flex gap-2">
            <Button 
              variant="secondary" 
              disabled={!hasPrev || loading}
              onClick={() => setPage(p => Math.max(1, p - 1))}
            >
              Prev
            </Button>
            <Button 
              variant="secondary" 
              disabled={!hasNext || loading}
              onClick={() => setPage(p => p + 1)}
            >
              Next
            </Button>
          </div>
          <div className="text-sm text-gray-500">
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
  );
}