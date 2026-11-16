// The header's search box (ADR 0034 §4; architecture 19 §2.1, §5.3): a plain form
// without JavaScript, and with it a suggesting box built on Codex's Lookup, loaded when
// the box is first used, which suggests entities and pages as the reader types.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test( 'without JavaScript the box is a form that searches', async ( { page } ) => {
	await page.goto( '/wiki/Project:Home' );
	const box = page.locator( '.ts-header' ).getByRole( 'searchbox', { name: 'Search' } );
	await box.fill( 'six' );
	await box.press( 'Enter' );
	await expect( page ).toHaveURL( /title=Special(:|%3A)Search&search=six/ );
	await expect( page.locator( '#content' ) ).toContainText( 'Six & more' );
} );

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'the box suggests entities and pages, and passes axe', async ( { page } ) => {
		const scripts = [];
		page.on( 'response', ( r ) => {
			if ( /\/ui\/assets\/.*\.js$/.test( r.url() ) ) {
				scripts.push( r.url() );
			}
		} );
		await page.goto( '/wiki/Project:Home' );
		// A reader who does not search downloads none of the component.
		expect( scripts.some( ( u ) => /\/mount-/.test( u ) ) ).toBe( false );

		await page.locator( '.ts-header input[type="search"]' ).click();
		await page.keyboard.type( 'six' );
		const live = page.locator( '#ts-typeahead' );
		await expect( live ).toBeVisible();
		// What was typed while the component loaded is kept.
		const box = live.getByRole( 'combobox', { name: 'Search' } );
		await expect( box ).toHaveValue( 'six' );
		const item = live.getByRole( 'option', { name: /Six & more/ } );
		await expect( item ).toBeVisible();
		// The last option leads to the full results.
		await expect( live.getByRole( 'option' ).last() ).toHaveText( 'Search for “six”' );
		expect( scripts.some( ( u ) => /\/mount-/.test( u ) ) ).toBe( true );

		const results = await new AxeBuilder( { page } )
			.include( '.ts-header' )
			.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
			.analyze();
		expect( results.violations.map(
			( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
		) ).toEqual( [] );

		// A suggestion goes to its page.
		await item.click();
		await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	} );

	test( 'an ID is offered as a Go to, the keys choose, and Enter searches', async ( { page } ) => {
		await page.goto( '/wiki/Project:Home' );
		await page.locator( '.ts-header input[type="search"]' ).click();
		await page.keyboard.type( 'Q6' );
		const live = page.locator( '#ts-typeahead' );
		const box = live.getByRole( 'combobox', { name: 'Search' } );
		// An ID is not a label: it is offered as a Go to, first.
		await expect( live.getByRole( 'option' ).first() ).toHaveText( 'Go to Q6' );
		// Enter with nothing chosen submits the form, as the plain one does.
		await box.fill( 'Six' );
		await expect( live.getByRole( 'option', { name: /Six & more/ } ) ).toBeVisible();
		await page.keyboard.press( 'Escape' );
		await box.press( 'Enter' );
		await expect( page ).toHaveURL( /title=Special(:|%3A)Search&search=Six/ );
		// The arrow keys choose a suggestion and Enter goes to it: the Go to lands on
		// the entity's page.
		await page.goto( '/wiki/Project:Home' );
		await page.locator( '.ts-header input[type="search"]' ).click();
		await page.keyboard.type( 'Q6' );
		await expect( page.locator( '#ts-typeahead' ).getByRole( 'option' ).first() )
			.toHaveText( 'Go to Q6' );
		await page.keyboard.press( 'ArrowDown' );
		await page.keyboard.press( 'Enter' );
		await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	} );
} );
