import type { NavigateFunction } from 'react-router';




import type { View } from './view';
import type { ViewOptions } from './viewOptions';
import type { NavigationHandler } from './navigationHandler';
export const createNavigationHandler = (navigate: NavigateFunction): NavigationHandler => {
  return (view: View, options?: ViewOptions): void => {
    switch (view) {
      case 'chat':
        void navigate('/', { state: options });
        break;
      case 'pair': {
        // Put resumeSessionId in URL search params (not just state) so that:
        // 1. The sidebar can read it to highlight the active session
        // 2. Page refresh preserves which session is active
        // 3. Browser back/forward navigation works correctly
        const searchParams: URLSearchParams = new URLSearchParams();
        if (options?.resumeSessionId) {
          searchParams.set('resumeSessionId', options.resumeSessionId);
        }
        const search: string = searchParams.toString();
        const url: string = search ? `/pair?${search}` : '/pair';
        void navigate(url, { state: options });
        break;
      }
      case 'settings':
        void navigate('/settings', { state: options });
        break;
      case 'sessions':
        void navigate('/sessions', { state: options });
        break;
      case 'schedules':
        void navigate('/schedules', { state: options });
        break;
      case 'recipes':
        void navigate('/recipes', { state: options });
        break;
      case 'skills':
        void navigate('/skills', { state: options });
        break;
      case 'permission':
        void navigate('/permission', { state: options });
        break;
      case 'ConfigureProviders':
        void navigate('/configure-providers', { state: options });
        break;
      case 'extensions':
        void navigate('/extensions', { state: options });
        break;
      default:
        void navigate('/', { state: options });
    }
  };
};

export type { View } from './view';
export type { ViewOptions } from './viewOptions';
export type { NavigationHandler } from './navigationHandler';
