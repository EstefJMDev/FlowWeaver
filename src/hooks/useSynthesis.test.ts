import { renderHook, act } from '@testing-library/react';
import { vi, describe, it, expect, beforeEach } from 'vitest';

const mockUnlisten = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(mockUnlisten),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

import { useSynthesis } from './useSynthesis';

describe('useSynthesis', () => {
  beforeEach(() => {
    mockUnlisten.mockClear();
  });

  it('limpia los tres listeners al desmontar el hook', async () => {
    const { unmount } = renderHook(() => useSynthesis('anchor-test'));
    await act(async () => {});
    unmount();
    // chunk + complete + error = 3 unlisten calls
    expect(mockUnlisten).toHaveBeenCalledTimes(3);
  });

  it('estado inicial es idle', () => {
    const { result } = renderHook(() => useSynthesis('anchor-test'));
    expect(result.current.state.status).toBe('idle');
  });
});
