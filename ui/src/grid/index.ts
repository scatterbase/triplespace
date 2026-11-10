/**
 * The grid components (ADR 0106 §1, §6, §7; 0045 §6, §7), loaded only on a page that has
 * a sheet data block: `Special:CreateManyEntities` for a signed-in viewer, and a Table
 * page the viewer may edit. Each page works without this script, as a text box and as a
 * drawn table.
 */

import { createApp } from 'vue';
import { Api } from '../edit/api';
import CreateGrid from './CreateGrid.vue';
import TableEditor from './TableEditor.vue';
import { msg, readSheetData } from './data';

const data = readSheetData( document );
if ( data && data.page === 'create' ) {
	const form = document.querySelector<HTMLFormElement>( '.ts-createentities__form' );
	const area = form?.elements.namedItem( 'rows' ) as HTMLTextAreaElement | null;
	if ( form && area ) {
		const host = document.createElement( 'div' );
		( area.closest( '.cdx-field' ) ?? area ).before( host );
		createApp( CreateGrid, { data, api: new Api(), form } ).mount( host );
	}
} else if ( data && data.page === 'table' ) {
	const grid = document.querySelector<HTMLElement>( '.ts-table' );
	if ( grid ) {
		const button = document.createElement( 'button' );
		button.type = 'button';
		button.className = 'cdx-button ts-table__edit';
		button.textContent = msg( data, 'ts-sheet-edit-table' );
		const host = document.createElement( 'div' );
		grid.before( button, host );
		button.addEventListener( 'click', () => {
			button.hidden = true;
			grid.hidden = true;
			const app = createApp( TableEditor, {
				data,
				api: new Api(),
				onCancel: () => {
					app.unmount();
					button.hidden = false;
					grid.hidden = false;
					button.focus();
				},
				onSaved: () => window.location.reload(),
			} );
			app.mount( host );
		} );
	}
}
