import * as React from 'react'
import type { ReactElement } from 'react'

import type { RouteComponent } from './types'

const isServerSide = typeof window === 'undefined'

interface BailoutToCSRProps {
  children: React.ReactNode
  reason: string
}

export function BailoutToCSR({ reason, children }: BailoutToCSRProps) {
  if (isServerSide) {
    throw new Error(reason)
  }

  return children
}

type ComponentModule<P = {}> = { default: React.ComponentType<P> }

// Normalize loader to return the module as form { default: Component } for `React.lazy`.
// Also for backward compatible since next/dynamic allows to resolve a component directly with loader
// Client component reference proxy need to be converted to a module.
function convertModule<P>(
  mod: React.ComponentType<P> | ComponentModule<P> | undefined,
): {
  default: React.ComponentType<P>
} {
  // Check "default" prop before accessing it, as it could be client reference proxy that could break it reference.
  // Cases:
  // mod: { default: Component }
  // mod: Component
  // mod: { default: proxy(Component) }
  // mod: proxy(Component)
  const hasDefault = mod && 'default' in mod
  return {
    default: hasDefault
      ? (mod as ComponentModule<P>).default
      : (mod as React.ComponentType<P>),
  }
}

type ImportFn = () => Promise<{ default: RouteComponent }>

interface DynamicOptions {
  ssr?: boolean
  loading?: React.ComponentType<any> | null
}

interface LoadableOptions extends DynamicOptions {
  loader?: () => Promise<React.ComponentType<any> | ComponentModule<any>>
}

export type LoadableFn<P = {}> = (
  opts: LoadableOptions,
) => React.ComponentType<P>

const defaultLoaderOptions: LoadableOptions = {
  ssr: true,
  loading: null,
  loader: () => Promise.resolve(convertModule(() => null)),
}

export function noSSR<P = {}>(
  LoadableInitializer: LoadableFn<P>,
  loadableOptions: DynamicOptions,
): React.ComponentType<P> {
  // This check is necessary to prevent react-loadable from initializing on the server
  if (!isServerSide) {
    return LoadableInitializer(loadableOptions)
  }

  if (!loadableOptions.loading) return () => null

  const Loading = loadableOptions.loading
  // This will only be rendered on the server side
  return () => <Loading />
}

const Loadable = (options: LoadableOptions) => {
  const opts = { ...defaultLoaderOptions, ...options }
  const Lazy = React.lazy(() => opts.loader().then(convertModule))
  const Loading = opts.loading

  function LoadableComponent(props: any): React.JSX.Element {
    const children = opts.ssr ? (
      <>
        {/* During SSR, we need to preload the CSS from the dynamic component to avoid flash of unstyled content */}
        {/*TODO: Preload here*/}
        <Lazy {...props} />
      </>
    ) : (
      <BailoutToCSR reason="tuono/dynamic">
        <Lazy {...props} />
      </BailoutToCSR>
    )
    return (
      <React.Suspense fallback={Loading ? <Loading /> : null}>
        {children}
      </React.Suspense>
    )
  }
  LoadableComponent.displayName = 'LoadableComponent'

  return LoadableComponent
}

export const dynamic2 = (
  importFn: ImportFn,
  opts?: DynamicOptions,
): React.JSX.Element => {
  if (typeof opts?.ssr === 'boolean' && !opts?.ssr) {
    return noSSR(Loadable, { ...opts, loader: importFn })
  }
  return Loadable({ ...opts, loader: importFn })
}

/**
 * Helper function to lazy load any component.
 *
 * The function acts exactly like React.lazy function but also renders the component on the server.
 * If you want to just load the component client side use directly the react's lazy function.
 *
 * It can be wrapped within a React.Suspense component in order to handle its loading state.
 */
// eslint-disable-next-line @typescript-eslint/no-unused-vars
export const dynamic = (importFn: ImportFn): React.JSX.Element => {
  /**
   *
   * This function is just a placeholder. The real work is done by the bundler.
   * The custom babel plugin will create two different bundles for the client and the server.
   *
   * The client will import the React's lazy function while the server will statically
   * import the file.
   *
   * Example:
   *
   * // User code
   * import { dynamic } from 'tuono'
   * const MyComponent = dynamic(() => import('./my-component'))
   *
   * // Client side generated code
   * import { lazy } from 'react'
   * const MyComponent = lazy(() => import('./my-component'))
   *
   * // Server side generated code
   * import MyComponent from './my-component'
   *
   * Check the `lazy-fn-vite-plugin` package for more
   */
  return <></>
}

export const __tuono__internal__lazyLoadComponent = (
  factory: ImportFn,
): RouteComponent => {
  let LoadedComponent: RouteComponent | undefined
  const LazyComponent = React.lazy(factory) as unknown as RouteComponent

  const loadComponent = (): Promise<void> =>
    factory().then((module) => {
      LoadedComponent = module.default
    })

  const Component = (
    props: React.ComponentProps<RouteComponent>,
  ): ReactElement =>
    React.createElement(LoadedComponent || LazyComponent, props)

  Component.preload = loadComponent

  return Component
}
