import type { Metadata } from 'next'
import { Geist, Geist_Mono } from 'next/font/google'
import AuthWrapper from '@/components/AuthWrapper'
import { UploadProvider } from '@/context/UploadContext'
import './globals.css'

const geistSans = Geist({
  variable: '--font-geist-sans',
  subsets: ['latin']
})

const geistMono = Geist_Mono({
  variable: '--font-geist-mono',
  subsets: ['latin']
})

export const metadata: Metadata = {
  title: 'Akane Admin WebUI',
  description: 'Admin interface for Akane video uploader and manager'
}

export default function RootLayout({
  children
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en'>
      <body className={`${geistSans.variable} ${geistMono.variable} antialiased`}>
        <AuthWrapper>
          <UploadProvider>{children}</UploadProvider>
        </AuthWrapper>
      </body>
    </html>
  )
}
