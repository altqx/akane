import Link from 'next/link'
import { usePathname } from 'next/navigation'

export default function Navbar() {
  const pathname = usePathname()

  const isActive = (path: string) => pathname === path

  return (
    <div className='navbar bg-base-100 mb-8 border-b border-base-300'>
      <div className='flex-1'>
        <Link href='/' className='btn btn-ghost text-xl'>
          Akane
        </Link>
      </div>
      <div className='flex-none'>
        <ul className='menu menu-horizontal px-1'>
          <li>
            <Link href='/' className={isActive('/') ? 'active' : ''}>
              Uploader
            </Link>
          </li>
          <li>
            <Link href='/videos' className={isActive('/videos') ? 'active' : ''}>
              Videos
            </Link>
          </li>
          <li>
            <Link href='/analytics' className={isActive('/analytics') ? 'active' : ''}>
              Analytics
            </Link>
          </li>
        </ul>
      </div>
    </div>
  )
}
