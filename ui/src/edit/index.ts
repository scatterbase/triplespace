/**
 * The editing components (ADR 0034 §4; 0003 §8), loaded only on a page that has an
 * edit data block: a signed-in viewer on an entity they may edit. Each edit button the
 * server wrote is hidden until this script shows it, so a page without script offers
 * nothing it cannot do.
 *
 * - The term editor edits the label, description and aliases in the interface language,
 *   in one `wbeditentity` with `baserevid`; then the `terms` region is fetched again and
 *   swapped in, and the title updated.
 * - The group editor edits every value of one property, with ranks, qualifiers and
 *   sources, in one `wbeditentity`; then the `statements/{P}` region is swapped in.
 *   **Add statement** picks a property and opens its group editor, and the new group's
 *   region is put before the button.
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
import GroupEditor from './GroupEditor.vue';
import PropertyPicker from './PropertyPicker.vue';
import { describe } from './describe';
import type { Snak } from './values';
import type { Statement } from './statements';

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
 * The IDs a group's editor shows labels for: its property, its qualifiers' and sources'
 * properties, and the entities its values name, units included.
 *
 * @param property The group's property
 * @param statements Its statements
 * @param conceptBase The site's concept base
 * @return The IDs
 */
function shown( property: string, statements: Statement[], conceptBase: string ): string[] {
	const ids = [ property ];
	const snak = ( s: Snak ): void => {
		ids.push( s.property );
		const v = s.datavalue?.value as Record<string, unknown> | undefined;
		if ( s.datavalue?.type === 'wikibase-entityid' && v ) {
			ids.push( String( v.id ) );
		}
		if ( s.datavalue?.type === 'quantity' && v && String( v.unit ).startsWith( conceptBase ) ) {
			ids.push( String( v.unit ).slice( conceptBase.length ) );
		}
	};
	for ( const st of statements ) {
		snak( st.mainsnak );
		Object.values( st.qualifiers ?? {} ).flat().forEach( snak );
		( st.references ?? [] ).forEach( ( r ) => Object.values( r.snaks ).flat().forEach( snak ) );
	}
	return ids;
}

/**
 * Opens a group's editor after `anchor`, hiding `region` (the group as drawn) while it
 * is open; for a new group, `region` is null and the saved group goes before `anchor`.
 *
 * @param doc The document
 * @param api The API client
 * @param data The edit data
 * @param property The group's property
 * @param anchor Where the editor goes
 * @param region The group's region, if the entity has one
 */
async function editGroup(
	doc: Document,
	api: Api,
	data: EditData,
	property: string,
	anchor: HTMLElement,
	region: HTMLElement | null,
): Promise<void> {
	const [ entity, lastrevid ] = await current( api, data );
	const claims = ( entity.claims ?? {} ) as Record<string, Statement[]>;
	const statements = claims[ property ] ?? [];
	const facts = await describe( api, data.lang, shown( property, statements, data.conceptBase ) );
	const datatype = statements[ 0 ]?.mainsnak.datatype ?? facts.datatypes[ property ] ?? 'string';
	const host = doc.createElement( 'div' );
	anchor.after( host );
	if ( region ) {
		region.hidden = true;
	}
	let app: App | null = null;
	const close = async ( saved: boolean ): Promise<void> => {
		app?.unmount();
		host.remove();
		if ( region ) {
			region.hidden = false;
		}
		if ( !saved ) {
			( region ?? anchor ).querySelector<HTMLElement>( '[data-ts-edit]' )?.focus();
			return;
		}
		const fresh = await refreshRegion( doc, data, `statements/${ property }`, {
			place: ( el ) => ( anchor.closest( '.ts-edit-bar' ) ?? anchor ).before( el ),
		} );
		if ( fresh ) {
			wire( doc, api, data, fresh );
			fresh.querySelector<HTMLElement>( '[data-ts-edit="group"]' )?.focus();
		}
	};
	app = createApp( GroupEditor, {
		data,
		api,
		conceptBase: data.conceptBase,
		property,
		propertyLabel: facts.labels[ property ] ?? property,
		datatype,
		statements,
		labels: facts.labels,
		datatypes: facts.datatypes,
		lastrevid,
		onSaved: () => close( true ),
		onCancel: () => close( false ),
		onRestart: async () => {
			await close( false );
			await editGroup( doc, api, data, property, anchor, region );
		},
	} );
	app.mount( host );
}

/**
 * Opens the property search of **Add statement**; the chosen property's group editor
 * follows, or its existing group's editor where the entity already uses it.
 *
 * @param doc The document
 * @param api The API client
 * @param data The edit data
 * @param button The button
 */
function addStatement( doc: Document, api: Api, data: EditData, button: HTMLElement ): void {
	const host = doc.createElement( 'div' );
	button.after( host );
	button.hidden = true;
	const app = createApp( PropertyPicker, {
		data,
		api,
		action: button.textContent ?? '',
		onChosen: async ( property: string ) => {
			app.unmount();
			host.remove();
			button.hidden = false;
			const region = doc.querySelector<HTMLElement>( `[data-region="statements/${ CSS.escape( property ) }"]` );
			const anchor = region?.querySelector<HTMLElement>( '[data-ts-edit="group"]' ) ?? button;
			await editGroup( doc, api, data, property, region || anchor, region );
		},
	} );
	app.mount( host );
	host.querySelector<HTMLButtonElement>( 'button' )?.click();
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
	const busy = ( b: HTMLButtonElement, work: () => Promise<void> ): void => {
		b.disabled = true;
		work().finally( () => {
			b.disabled = false;
		} );
	};
	for ( const b of root.querySelectorAll<HTMLButtonElement>( 'button[data-ts-edit]' ) ) {
		b.hidden = false;
		const kind = b.dataset.tsEdit;
		b.addEventListener( 'click', () => {
			if ( kind === 'terms' ) {
				busy( b, () => editTerms( doc, api, data ) );
			} else if ( kind === 'group' ) {
				const region = b.closest<HTMLElement>( '[data-region]' );
				busy( b, () => editGroup( doc, api, data, b.dataset.property ?? '', region ?? b, region ) );
			} else if ( kind === 'add-statement' ) {
				addStatement( doc, api, data, b );
			}
		} );
	}
}

const editData = readData( document );
if ( editData ) {
	wire( document, new Api(), editData, document );
}
