/**
 * The editing components (ADR 0034 §4; 0003 §8), loaded only on a page that has an
 * edit data block: a signed-in viewer on an entity they may edit. Each edit button the
 * server wrote is hidden until this script shows it, so a page without script offers
 * nothing it cannot do.
 *
 * - The term editor edits the label, description and aliases in the interface language,
 *   in one `wbeditentity` with `baserevid`; then the `terms` region is fetched again and
 *   swapped in, and the title updated.
 */

import { createApp } from 'vue';
import type { App } from 'vue';
import { Api } from './api';
import { readData } from './data';
import type { EditData } from './data';
import { refreshRegion } from './regions';
import { termsOf } from './terms';
import type { Terms } from './terms';
import TermsEditor from './TermsEditor.vue';

/**
 * The entity as it is now, and its revision.
 *
 * @param api The API client
 * @param data The edit data
 * @return The entity's JSON and its last revision ID
 */
async function current( api: Api, data: EditData ): Promise<[ Record<string, unknown>, number ]> {
	const body = await api.get( {
		action: 'wbgetentities',
		ids: data.id,
		props: 'info|labels|descriptions|aliases|claims',
		languages: data.lang,
	} );
	const entities = body.entities as Record<string, Record<string, unknown>>;
	const entity = entities[ data.id ] ?? {};
	return [ entity, Number( entity.lastrevid ?? 0 ) ];
}

/**
 * Puts a saved label in the page's heading and the document's title.
 *
 * @param doc The document
 * @param data The edit data
 * @param label The label, or empty for none
 */
function retitle( doc: Document, data: EditData, label: string ): void {
	const h1 = doc.querySelector( '.ts-page__title' );
	if ( h1 ) {
		h1.textContent = label || data.id;
		h1.removeAttribute( 'lang' );
		h1.removeAttribute( 'dir' );
	}
	const site = doc.title.split( ' – ' ).pop() ?? '';
	doc.title = `${ label ? `${ label } (${ data.id })` : data.id } – ${ site }`;
}

/**
 * Opens the term editor in place of the terms region.
 *
 * @param doc The document
 * @param api The API client
 * @param data The edit data
 */
async function editTerms( doc: Document, api: Api, data: EditData ): Promise<void> {
	const region = doc.querySelector<HTMLElement>( '[data-region="terms"]' );
	if ( !region ) {
		return;
	}
	const [ entity, lastrevid ] = await current( api, data );
	const host = doc.createElement( 'div' );
	region.after( host );
	region.hidden = true;
	let app: App | null = null;
	const close = async ( saved: Terms | null ): Promise<void> => {
		app?.unmount();
		host.remove();
		if ( saved ) {
			retitle( doc, data, saved.label );
			const fresh = await refreshRegion( doc, data, 'terms' );
			if ( fresh ) {
				wire( doc, api, data, fresh );
				fresh.querySelector<HTMLElement>( '[data-ts-edit="terms"]' )?.focus();
			}
		} else {
			region.hidden = false;
			region.querySelector<HTMLElement>( '[data-ts-edit="terms"]' )?.focus();
		}
	};
	app = createApp( TermsEditor, {
		data,
		api,
		initial: termsOf( entity, data.lang ),
		lastrevid,
		onSaved: ( t: Terms ) => close( t ),
		onCancel: () => close( null ),
		onRestart: async () => {
			await close( null );
			await editTerms( doc, api, data );
		},
	} );
	app.mount( host );
}

/**
 * Shows the edit buttons under `root` and gives them their editors.
 *
 * @param doc The document
 * @param api The API client
 * @param data The edit data
 * @param root Where to look
 */
export function wire( doc: Document, api: Api, data: EditData, root: ParentNode ): void {
	for ( const b of root.querySelectorAll<HTMLButtonElement>( 'button[data-ts-edit="terms"]' ) ) {
		b.hidden = false;
		b.addEventListener( 'click', () => {
			b.disabled = true;
			editTerms( doc, api, data ).finally( () => {
				b.disabled = false;
			} );
		} );
	}
}

const editData = readData( document );
if ( editData ) {
	wire( document, new Api(), editData, document );
}
