import Link from 'next/link'
import { usePathname } from 'next/navigation'

export default function Navbar() {
  const pathname = usePathname()

  const isActive = (path: string) => pathname === path

  return (
    <nav className='mb-8 border-b border-border pb-4'>
      <div className='flex gap-6'>
        <Link
          href='/'
          className={`text-sm font-medium transition-colors hover:text-primary ${
            isActive('/') ? 'text-primary' : 'text-muted-foreground'
          }`}
        >
          Uploader
        </Link>
        <Link
          href='/videos'
          className={`text-sm font-medium transition-colors hover:text-primary ${
            isActive('/videos') ? 'text-primary' : 'text-muted-foreground'
          }`}
        >
          Videos
        </Link>
        <Link
          href='/analytics'
          className={`text-sm font-medium transition-colors hover:text-primary ${
            isActive('/analytics') ? 'text-primary' : 'text-muted-foreground'
          }`}
        >
          Analytics
        </Link>
      </div>
    </nav>
  )
}
