/**
 * Swapping a region of the page for the server's rendering of it after a save (ADR 0034
 * §5; 0057 §8): `action=render` on the page's own URL, with the page's build. A server
 * on another build answers `409`, and the page reloads instead of mixing markup with
 * styles it was not built for.
 */

import type { EditData } from './data';
import type { Fetcher } from './api';

/** What happens when the server is on another build; a test passes its own. */
export type Reload = () => void;

/**
 * The region's URL.
 *
 * @param data The edit data
 * @param region The region name: `terms`, `statements/P31`
 * @return The URL
 */
export function regionUrl( data: EditData, region: string ): string {
	const q = new URLSearchParams( {
		title: data.title,
		action: 'render',
		region,
		uselang: data.lang,
	} );
	return `/w/index.php?${ q.toString() }`;
}

/** How a region is fetched and placed; a test passes its own fetch and reload. */
export interface RefreshOptions {
	/** The fetch function. */
	fetcher?: Fetcher;
	/** What to do on a build mismatch. */
	reload?: Reload;
	/** Where to put a region the page does not have yet. */
	place?: ( el: Element ) => void;
}

/**
 * Fetches a region and puts it in place of the element that holds it now.
 *
 * @param doc The document
 * @param data The edit data
 * @param region The region name
 * @param options How to fetch and place it
 * @return The new element, or null when the page reloads
 */
export async function refreshRegion(
	doc: Document,
	data: EditData,
	region: string,
	options: RefreshOptions = {},
): Promise<Element | null> {
	const fetcher = options.fetcher ?? ( ( i: string, init?: RequestInit ) => fetch( i, init ) );
	const reload = options.reload ?? ( () => doc.defaultView?.location.reload() );
	const r = await fetcher( regionUrl( data, region ), {
		credentials: 'same-origin',
		cache: 'no-cache',
		headers: { 'X-Triplespace-UI-Build': data.build },
	} );
	if ( r.status === 409 ) {
		reload();
		return null;
	}
	if ( !r.ok && r.status !== 404 ) {
		throw new Error( `HTTP ${ r.status }` );
	}
	const template = doc.createElement( 'template' );
	template.innerHTML = ( await r.text() ).trim();
	const fresh = template.content.firstElementChild;
	const old = doc.querySelector( `[data-region="${ CSS.escape( region ) }"]` );
	if ( !fresh ) {
		old?.remove();
		return null;
	}
	if ( old ) {
		old.replaceWith( fresh );
	} else if ( options.place ) {
		options.place( fresh );
	}
	return fresh;
}
