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

/**
 * Fetches a region and puts it in place of the element that holds it now.
 *
 * @param doc The document
 * @param data The edit data
 * @param region The region name
 * @param fetcher The fetch function
 * @param reload What to do on a build mismatch
 * @return The new element, or null when the page reloads
 */
export async function refreshRegion(
	doc: Document,
	data: EditData,
	region: string,
	fetcher: Fetcher = ( i, init ) => fetch( i, init ),
	reload: Reload = () => doc.defaultView?.location.reload(),
): Promise<Element | null> {
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
	}
	return fresh;
}
