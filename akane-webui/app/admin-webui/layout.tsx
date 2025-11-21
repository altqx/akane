'use client';

import { useState } from 'react';
import Input from '@/components/Input';
import Button from '@/components/Button';

export default function AdminLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const [isAuthenticated, setIsAuthenticated] = useState(() => {
    if (typeof window !== 'undefined') {
      const auth = localStorage.getItem('admin_auth');
      return auth === 'true';
    }
    return false;
  });
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');

  const handleLogin = (e: React.FormEvent) => {
    e.preventDefault();
    // Hardcoded password as requested
    if (password === 'akane-admin-2025') {
      localStorage.setItem('admin_auth', 'true');
      setIsAuthenticated(true);
      setError('');
    } else {
      setError('Invalid password');
    }
  };

  if (!isAuthenticated) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50">
        <div className="w-full max-w-md rounded-lg bg-white p-8 shadow-md">
          <h1 className="mb-6 text-center text-2xl font-bold text-gray-900">Admin Access</h1>
          <form onSubmit={handleLogin} className="flex flex-col gap-4">
            <Input
              type="password"
              label="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter admin password"
            />
            {error && <div className="text-sm text-red-600">{error}</div>}
            <Button type="submit">Login</Button>
          </form>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}