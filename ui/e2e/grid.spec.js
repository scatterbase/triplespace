// The grid (ADR 0106 §1, §6, §7; 0045 §6, §7), with JavaScript: the table of
// Special:CreateManyEntities edited as a grid over the text the form posts, an uploaded
// file mapped into it, and a Table page's cells and rows edited in place.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form, returning to `title`.
 *
 * @param {Object} page A Playwright page
 * @param {string} title The page to return to
 */
async function logIn( page, title ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ title }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await page.waitForURL( `**/wiki/${ title }` );
}

/**
 * A write to the Action API as the signed-in viewer.
 *
 * @param {Object} page A Playwright page
 * @param {Object} params The parameters, without the token
 * @return {Promise<Object>} The response body
 */
async function write( page, params ) {
	const tokens = await page.request.get( '/w/api.php?action=query&meta=tokens&type=csrf&format=json&formatversion=2' );
	const token = ( await tokens.json() ).query.tokens.csrftoken;
	const r = await page.request.post( '/w/api.php', {
		form: { ...params, token, format: 'json', formatversion: '2' },
		headers: { Origin: new URL( page.url() ).origin }
	} );
	return r.json();
}

/**
 * axe's violations within a part of the page, as text.
 *
 * @param {Object} page A Playwright page
 * @param {string} selector The part
 * @return {Promise<string[]>}
 */
async function axe( page, selector ) {
	const results = await new AxeBuilder( { page } ).include( selector ).analyze();
	return results.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.html ).join( ' | ' ) }` );
}

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'the table is edited as a grid over its text, checked, and imported', async ( { page } ) => {
		await logIn( page, 'Special:CreateManyEntities' );
		const grid = page.locator( '.ts-sheet-create' );
		await expect( grid.locator( '.ts-sheet__table' ) ).toBeVisible();
		// The grid stands in for the text box, which still holds what the form posts.
		await expect( page.locator( 'textarea[name="rows"]' ) ).toBeHidden();

		await page.getByLabel( 'Label, row 1' ).fill( 'Grid item one' );
		await page.getByLabel( 'Description, row 1' ).fill( 'made in the grid' );
		// A property column, picked by its label.
		await grid.getByRole( 'button', { name: 'Add column' } ).click();
		await grid.getByRole( 'combobox', { name: 'Property' } ).fill( 'instance' );
		await page.getByRole( 'option', { name: /instance of/ } ).click();
		await grid.getByRole( 'button', { name: 'Add', exact: true } ).click();
		await page.getByLabel( 'P2, row 1' ).fill( 'Q2' );
		// A second row, pasted as a spreadsheet copies it, pointing at the first.
		await grid.getByRole( 'button', { name: 'Add row' } ).click();
		await page.getByLabel( 'Label, row 2' ).focus();
		await page.evaluate( () => {
			const data = new DataTransfer();
			data.setData( 'text/plain', 'Grid item two\tpasted in\t\tnonsense\n' );
			document.activeElement.dispatchEvent( new ClipboardEvent( 'paste', { clipboardData: data, bubbles: true, cancelable: true } ) );
		} );
		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue(
			'label,description,aliases,P2\nGrid item one,made in the grid,,Q2\nGrid item two,pasted in,,nonsense\n'
		);

		// The page's check, shown on the rows.
		await grid.getByRole( 'button', { name: 'Check rows now' } ).click();
		await expect( grid.locator( '.ts-sheet__status' ).first() ).toHaveText( 'new' );
		await expect( grid.locator( '.ts-sheet__status' ).nth( 1 ) ).toHaveText( 'invalid (1)' );
		await expect( grid.locator( '.ts-sheet__summary' ) ).toHaveText( 'Create 1 item, add to 0, skip 1' );
		// The property is named, and the item the first row's cell holds.
		await expect( grid.getByRole( 'columnheader', { name: /instance of/ } ) ).toBeVisible();
		await expect( grid.locator( '.ts-sheet__note' ).first() ).not.toHaveText( '' );
		await grid.getByLabel( 'Only rows with problems' ).check();
		await expect( grid.locator( '.ts-sheet__row' ) ).toHaveCount( 1 );
		await grid.getByLabel( 'Only rows with problems' ).uncheck();
		expect( await axe( page, '.ts-sheet-create' ) ).toEqual( [] );

		// Fixed, then as text and back.
		await page.getByLabel( /, row 2$/ ).last().fill( 'row:1' );
		await grid.getByRole( 'button', { name: 'Text', exact: true } ).click();
		await expect( page.locator( 'textarea[name="rows"]' ) ).toBeVisible();
		await grid.getByRole( 'button', { name: 'Grid', exact: true } ).click();
		await expect( page.getByLabel( 'Label, row 2' ) ).toHaveValue( 'Grid item two' );

		// The form's own check and confirmation write the rows.
		await page.getByRole( 'button', { name: 'Check the rows' } ).click();
		await page.getByRole( 'button', { name: 'Create 2 items, add to 0, skip 0' } ).click();
		await expect( page.getByText( '2 items created, 0 added to, 0 not written.' ) ).toBeVisible();
	} );

	test( 'an uploaded file is mapped into the grid', async ( { page } ) => {
		await logIn( page, 'Special:CreateManyEntities' );
		await page.locator( 'input[type="file"]' ).setInputFiles( {
			name: 'items.csv',
			mimeType: 'text/csv',
			buffer: Buffer.from( 'Name,instance of,junk,Source\nMapped one,Q2,x,https://example.org/1\nMapped two,Q2,y,\n' )
		} );
		const map = page.locator( '.ts-sheet-map' );
		await expect( map.getByRole( 'heading', { name: 'Map the file\'s columns' } ) ).toBeVisible();
		// What is evident is suggested: a name is a label, a property's label that property.
		await expect( map.getByLabel( 'What “Name” becomes' ) ).toHaveValue( 'label' );
		await expect( map.getByLabel( 'What “instance of” becomes' ) ).toHaveValue( 'property' );
		await expect( map.getByLabel( 'What “junk” becomes' ) ).toHaveValue( 'ignore' );
		// The source is a value the reference on every statement reads.
		await map.getByLabel( 'What “Source” becomes' ).selectOption( 'reads' );
		await map.getByRole( 'row', { name: /Source/ } ).getByRole( 'combobox', { name: 'Property' } ).fill( 'P11' );
		await page.getByRole( 'option', { name: /P11/ } ).first().click();
		expect( await axe( page, '.ts-sheet-map' ) ).toEqual( [] );
		await map.getByRole( 'button', { name: 'Put the rows in the table' } ).click();

		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue(
			'label,P2,Source\nMapped one,Q2,https://example.org/1\nMapped two,Q2,\n'
		);
		await expect( page.locator( 'textarea[name="reference"]' ) ).toHaveValue( 'P11:{Source}' );
		await expect( page.getByLabel( 'Label, row 2' ) ).toHaveValue( 'Mapped two' );
		// The file is not uploaded as well: the grid holds its rows.
		await expect( page.locator( 'input[type="file"]' ) ).toHaveValue( '' );
	} );

	test( 'a table\'s cells and rows are edited in its grid', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		const made = await write( page, {
			action: 'wbeditentity',
			new: 'item',
			data: JSON.stringify( { labels: { en: { language: 'en', value: 'Table row one' } } } )
		} );
		const id = made.entity.id;
		// A quantity property and an item to be its unit.
		const pages = ( await write( page, {
			action: 'wbeditentity',
			new: 'property',
			data: JSON.stringify( { labels: { en: { language: 'en', value: 'number of pages' } }, datatype: 'quantity' } )
		} ) ).entity.id;
		const unit = ( await write( page, {
			action: 'wbeditentity',
			new: 'item',
			data: JSON.stringify( { labels: { en: { language: 'en', value: 'page (unit)' } } } )
		} ) ).entity.id;
		const saved = await write( page, {
			action: 'edit',
			title: 'Table:E2E grid',
			text: JSON.stringify( {
				version: 1,
				rows: { ids: [ id ] },
				columns: [
					{ field: 'label', language: 'en' },
					{ field: 'description', language: 'en' },
					{ field: 'statements', property: 'P2' },
					{ field: 'statements', property: pages }
				]
			} )
		} );
		expect( saved.edit.result ).toBe( 'Success' );

		await page.goto( '/wiki/Table:E2E_grid' );
		await page.getByRole( 'button', { name: 'Edit the table' } ).click();
		const editor = page.locator( '.ts-sheet-table' );
		await expect( page.getByLabel( `Label, row ${ id }` ) ).toHaveValue( 'Table row one' );
		expect( await axe( page, '.ts-sheet-table' ) ).toEqual( [] );

		// A quantity's unit is picked by search, and written after the amount as its ID.
		await page.getByLabel( `number of pages, row ${ id }`, { exact: true } ).fill( '320' );
		await editor.getByRole( 'combobox', { name: `Find a unit for number of pages, row ${ id }` } ).fill( 'page' );
		await page.getByRole( 'option', { name: /^page/ } ).first().click();
		await expect( page.getByLabel( `number of pages, row ${ id }`, { exact: true } ) ).toHaveValue( `320 ${ unit }` );

		// Two cells of the row, and a new item beneath it.
		await page.getByLabel( `Description, row ${ id }` ).fill( 'edited in the table' );
		await page.getByLabel( `instance of, row ${ id }` ).fill( 'Q2' );
		await editor.getByRole( 'button', { name: 'New item' } ).click();
		await page.getByLabel( 'Label, row new item' ).fill( 'Made from the table' );
		await editor.getByLabel( 'Summary' ).fill( 'e2e' );
		await editor.getByRole( 'button', { name: 'Save', exact: true } ).click();

		// Saved, the page is drawn again with the row's values and the new row.
		await expect( page.locator( '.ts-table__grid' ) ).toContainText( 'edited in the table' );
		await expect( page.locator( '.ts-table__grid' ) ).toContainText( 'Made from the table' );
		await expect( page.locator( '.ts-table__grid tbody tr' ) ).toHaveCount( 2 );

		// One change on the entity for both cells, summarized and tagged with the table.
		const revisions = await page.request.get(
			`/w/api.php?action=query&prop=revisions&titles=Item:${ id }&rvprop=comment|tags&rvlimit=1&format=json&formatversion=2`
		);
		const rev = ( await revisions.json() ).query.pages[ 0 ].revisions[ 0 ];
		expect( rev.comment ).toContain( 'Edited via [[Table:E2E grid]]: e2e' );
		expect( rev.tags.some( ( t ) => /^table:\d+$/.test( t ) ) ).toBe( true );
		const entity = await page.request.get( `/w/api.php?action=wbgetentities&ids=${ id }&format=json&formatversion=2` );
		const e = ( await entity.json() ).entities[ id ];
		expect( e.descriptions.en.value ).toBe( 'edited in the table' );
		expect( e.claims.P2[ 0 ].mainsnak.datavalue.value.id ).toBe( 'Q2' );
		expect( e.claims[ pages ][ 0 ].mainsnak.datavalue.value ).toMatchObject( { amount: '+320' } );
		expect( e.claims[ pages ][ 0 ].mainsnak.datavalue.value.unit.endsWith( `/${ unit }` ) ).toBe( true );
		// Read again, the cell shows the amount with its unit's label.
		await expect( page.locator( '.ts-table__grid' ) ).toContainText( '320 page (unit)' );

		// The one value changed keeps its statement; a row removed leaves its entity.
		const guid = e.claims.P2[ 0 ].id;
		await page.getByRole( 'button', { name: 'Edit the table' } ).click();
		await page.getByLabel( `instance of, row ${ id }` ).fill( 'Q3' );
		await editor.getByRole( 'button', { name: /^Remove row Q\d+$/ } ).last().click();
		await editor.getByRole( 'button', { name: 'Save', exact: true } ).click();
		await expect( page.locator( '.ts-table__grid tbody tr' ) ).toHaveCount( 1 );
		const after = await page.request.get( `/w/api.php?action=wbgetentities&ids=${ id }&format=json&formatversion=2` );
		const claims = ( await after.json() ).entities[ id ].claims.P2;
		expect( claims ).toHaveLength( 1 );
		expect( claims[ 0 ].id ).toBe( guid );
		expect( claims[ 0 ].mainsnak.datavalue.value.id ).toBe( 'Q3' );
	} );
} );

test( 'without JavaScript the table is a text box and a Table page a drawn table', async ( { page } ) => {
	await logIn( page, 'Special:CreateManyEntities' );
	await expect( page.locator( 'textarea[name="rows"]' ) ).toBeVisible();
	await expect( page.locator( '.ts-sheet-create' ) ).toHaveCount( 0 );
} );
