import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'export',
  basePath: '/admin-webui',
  distDir: 'dist',
  images: {
    unoptimized: true
  },
  // Rewrites are only for development (next dev).
  // In production (static export), these are ignored, but the files will be served
  // by the backend on the same origin, so relative paths work.
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://localhost:3000/api/:path*'
      },
      {
        source: '/hls/:path*',
        destination: 'http://localhost:3000/hls/:path*'
      },
      {
        source: '/player/:path*',
        destination: 'http://localhost:3000/player/:path*'
      }
    ]
  }
}

export default nextConfig
