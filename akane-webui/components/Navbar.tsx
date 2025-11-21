import Link from 'next/link';
import { usePathname } from 'next/navigation';

export default function Navbar() {
  const pathname = usePathname();

  const isActive = (path: string) => pathname === path;

  return (
    <nav className="mb-8 border-b border-gray-200 pb-4">
      <div className="flex gap-6">
        <Link 
          href="/admin-webui" 
          className={`text-sm font-medium transition-colors hover:text-black ${
            isActive('/admin-webui') ? 'text-black' : 'text-gray-500'
          }`}
        >
          Uploader
        </Link>
        <Link 
          href="/admin-webui/videos" 
          className={`text-sm font-medium transition-colors hover:text-black ${
            isActive('/admin-webui/videos') ? 'text-black' : 'text-gray-500'
          }`}
        >
          Videos
        </Link>
      </div>
    </nav>
  );
}