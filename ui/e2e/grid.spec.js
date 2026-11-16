// The grid (ADR 0106 §1, §6, §7, A9; 0045 §6, §7), with JavaScript: the table of
// Special:CreateManyEntities edited as a grid over the text the form posts, checked and
// then imported by the form's one button; a file mapped into it from
// Special:CreateEntitiesFromFile; and a Table page's cells and rows edited in place.
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

		// The table grows from its edges: no bar above it, and no switch to the text.
		await expect( grid.locator( '.ts-sheet__bar' ) ).toHaveCount( 0 );
		await expect( grid.getByRole( 'button', { name: 'Text', exact: true } ) ).toHaveCount( 0 );
		await expect( page.locator( 'input[type="file"]' ) ).toHaveCount( 0 );

		// The form's one button checks first: each row's status is shown on it.
		const form = page.locator( '.ts-createentities__form' );
		await form.getByRole( 'button', { name: 'Check the rows' } ).click();
		await expect( grid.locator( '.ts-sheet__status' ).first() ).toHaveText( 'new' );
		await expect( grid.locator( '.ts-sheet__status' ).nth( 1 ) ).toHaveText( 'invalid (1)' );
		await expect( grid.locator( '.ts-sheet__summary' ) ).toHaveText( 'Create 1 item, add to 0, skip 1' );
		// Checked, the button says what pressing it again writes.
		await expect( form.getByRole( 'button', { name: 'Create 1 item, add to 0, skip 1' } ) ).toBeVisible();
		// The property is named, and the item the first row's cell holds.
		await expect( grid.getByRole( 'columnheader', { name: /instance of/ } ) ).toBeVisible();
		await expect( grid.locator( '.ts-sheet__note' ).first() ).not.toHaveText( '' );
		await grid.getByLabel( 'Only rows with problems' ).check();
		await expect( grid.locator( '.ts-sheet__row' ) ).toHaveCount( 1 );
		await grid.getByLabel( 'Only rows with problems' ).uncheck();
		expect( await axe( page, '.ts-createentities__form' ) ).toEqual( [] );

		// A change makes the check stale, and the button checks again.
		await grid.getByRole( 'textbox', { name: /, row 2$/ } ).last().fill( 'row:1' );
		await expect( form.getByRole( 'button', { name: 'Check the rows' } ) ).toBeVisible();

		// A statement on every item, picked from a list where the text box was.
		await expect( page.locator( 'textarea[name="shared"]' ) ).toBeHidden();
		await form.getByRole( 'button', { name: 'Add statement' } ).click();
		await form.getByRole( 'combobox', { name: 'Property of line 1' } ).fill( 'P11' );
		await page.getByRole( 'option', { name: /P11/ } ).first().click();
		await form.getByLabel( 'Value of line 1' ).fill( 'https://example.org/every' );
		await expect( page.locator( 'textarea[name="shared"]' ) ).toHaveValue( 'P11:https://example.org/every' );
		// It stands as a locked column at the grid's edge, which a row unlocks for itself:
		// the cell is then the table's own, and replaces the shared statement there.
		await expect( grid.getByRole( 'columnheader', { name: /on every item/ } ) ).toBeVisible();
		await expect( grid.locator( '.ts-sheet__locked' ) ).toHaveCount( 2 );
		await grid.getByRole( 'button', { name: /^Change .* for row 2 alone$/ } ).click();
		await grid.getByRole( 'textbox', { name: /, row 2$/ } ).last().fill( 'https://example.org/two' );
		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue(
			'label,description,aliases,P2,P11\nGrid item one,made in the grid,,Q2,\nGrid item two,pasted in,,row:1,https://example.org/two\n'
		);
		await expect( grid.locator( '.ts-sheet__locked' ) ).toHaveCount( 1 );

		// A statement's own reference is opened from a chip on its cell, and is held in
		// the text as a column the grid does not show.
		const chip = grid.getByRole( 'button', { name: /Reference of .*, row 1$/ } ).first();
		await expect( chip ).toHaveText( '+ ref' );
		await chip.click();
		const own = grid.locator( '.ts-sheet__own' );
		await own.getByRole( 'button', { name: 'Add reference value' } ).click();
		await own.getByRole( 'combobox', { name: 'Property of line 1' } ).fill( 'P11' );
		await page.getByRole( 'option', { name: /P11/ } ).first().click();
		await own.getByLabel( 'Value of line 1' ).fill( 'https://example.org/own' );
		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue(
			'label,description,aliases,P2,P11,P2/ref:P11\nGrid item one,made in the grid,,Q2,,https://example.org/own\nGrid item two,pasted in,,row:1,https://example.org/two,\n'
		);
		await expect( chip ).toHaveText( 'ref · 1' );
		await expect( grid.getByRole( 'columnheader' ) ).toHaveCount( 7 );
		expect( await axe( page, '.ts-createentities__form' ) ).toEqual( [] );
		await own.getByRole( 'button', { name: 'Done' } ).click();

		// The table is kept under a title of its own, not as a draft.
		await form.getByLabel( 'Table name' ).fill( 'E2E grid run' );
		await form.getByLabel( /^Keep it as a table of its own/ ).check();

		// Checked again, the same button writes the rows: no page in between.
		await form.getByRole( 'button', { name: 'Check the rows' } ).click();
		await form.getByRole( 'button', { name: 'Create 2 items, add to 0, skip 0' } ).click();
		await expect( page.getByText( '2 items created, 0 added to, 0 not written.' ) ).toBeVisible();
		// The run left a Table page, and its job is closed, not running.
		const kept = page.locator( '.ts-createentities__draft a' );
		await expect( kept ).toHaveText( 'Table:E2E grid run' );
		await page.locator( '.ts-createentities a[href^="/wiki/Special:Jobs/"]' ).click();
		await expect( page.locator( '#content' ) ).toContainText( 'Closed' );
		await page.goto( '/wiki/Table:E2E_grid_run' );
		await expect( page.locator( '.ts-table__grid' ) ).toContainText( 'Grid item two' );
		// The first item's statement carries its own reference; the second, its own value
		// of the statement every item shares.
		const ids = await page.locator( '.ts-table__grid tbody th a' ).allTextContents();
		const got = await page.request.get( `/w/api.php?action=wbgetentities&ids=${ ids.join( '|' ) }&format=json&formatversion=2` );
		const entities = Object.values( ( await got.json() ).entities );
		const one = entities.find( ( e ) => e.labels.en.value === 'Grid item one' );
		const two = entities.find( ( e ) => e.labels.en.value === 'Grid item two' );
		expect( one.claims.P2[ 0 ].references[ 0 ].snaks.P11[ 0 ].datavalue.value ).toBe( 'https://example.org/own' );
		expect( one.claims.P11[ 0 ].mainsnak.datavalue.value ).toBe( 'https://example.org/every' );
		expect( two.claims.P11.map( ( c ) => c.mainsnak.datavalue.value ) ).toEqual( [ 'https://example.org/two' ] );
	} );

	test( 'an uploaded file is mapped into the grid', async ( { page } ) => {
		await logIn( page, 'Special:CreateEntitiesFromFile' );
		// The file is read when it is chosen, so the page's own button is not shown.
		await expect( page.getByRole( 'button', { name: 'Read the file' } ) ).toBeHidden();
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

		// The table opens in the grid of the page that checks and writes it.
		await page.waitForURL( '**/wiki/Special:CreateManyEntities' );
		await expect( page.getByText( 'The table below was read for you.' ) ).toBeVisible();
		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue(
			'label,P2,Source\nMapped one,Q2,https://example.org/1\nMapped two,Q2,\n'
		);
		await expect( page.locator( 'textarea[name="reference"]' ) ).toHaveValue( 'P11:{Source}' );
		await expect( page.getByLabel( 'Label, row 2' ) ).toHaveValue( 'Mapped two' );
		// The reference reads the column, shown as a line of the list beside the grid.
		const form = page.locator( '.ts-createentities__form' );
		await expect( form.getByLabel( 'Where the value of line 1 comes from' ) ).toHaveValue( 'Source' );
	} );

	test( 'a file of QuickStatements commands is read as the commands\' page reads them', async ( { page } ) => {
		await logIn( page, 'Special:CreateEntitiesFromFile' );
		await page.locator( 'input[type="file"]' ).setInputFiles( {
			name: 'batch.qs',
			mimeType: 'text/plain',
			buffer: Buffer.from( 'CREATE\nLAST|Len|"From commands"\nLAST|P2|Q2\n' )
		} );
		await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue( 'label,P2\nFrom commands,Q2\n' );
		await expect( page.getByLabel( 'Label, row 1' ) ).toHaveValue( 'From commands' );
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
	// The check is a page of its own, with the confirmation on it.
	await page.locator( 'textarea[name="rows"]' ).fill( 'label\nWithout script\n' );
	await page.getByRole( 'button', { name: 'Check the rows' } ).click();
	await expect( page.getByRole( 'heading', { name: 'The rows as they will be written' } ) ).toBeVisible();
	await expect( page.getByRole( 'button', { name: 'Create 1 item, add to 0, skip 0' } ) ).toBeVisible();

	// QuickStatements commands are read into the same table, and a removal is refused.
	await page.goto( '/wiki/Special:QuickStatements' );
	await page.getByLabel( 'Commands' ).fill( '-Q2|P2|Q3' );
	await page.getByRole( 'button', { name: 'Read the commands' } ).click();
	await expect( page.getByText( 'It removes something, which this page cannot do.' ) ).toBeVisible();
	await page.getByLabel( 'Commands' ).fill( 'CREATE\nLAST|Len|"Commanded"\nLAST|P2|Q2' );
	await page.getByRole( 'button', { name: 'Read the commands' } ).click();
	await expect( page.locator( 'textarea[name="rows"]' ) ).toHaveValue( 'label,P2\nCommanded,Q2\n' );
} );
