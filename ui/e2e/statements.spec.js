// Editing statements with JavaScript (ADR 0003 §8; web-frontend plan Phase 3): the
// milestone-1 acceptance edits through the group editor, on Item:Q1, which starts with no
// statements: a claim created through Add statement, a value set, a qualifier and a
// source added, a value removed. After every save the group's region is the server's
// own rendering. Requests alternate between the two web replicas.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test.use( { javaScriptEnabled: true } );

const strip = ( html ) => html.replace( /\s(hidden|disabled)(="")?(?=[\s>])/g, '' ).trim();

/**
 * Checks that a group's region on the page is what the server renders for it now.
 *
 * @param {Object} page A Playwright page
 * @param {string} property The group's property
 */
async function matchesServer( page, property ) {
	const region = page.locator( `[data-region="statements/${ property }"]` );
	const swapped = strip( await region.evaluate( ( el ) => el.outerHTML ) );
	const r = await page.request.get( `/w/index.php?title=Item:Q1&action=render&region=statements/${ property }&uselang=en` );
	expect( swapped ).toBe( strip( await r.text() ) );
}

/**
 * Picks an entity in a lookup by typing and choosing the option with its label.
 *
 * @param {Object} page A Playwright page
 * @param {Object} input The lookup's input
 * @param {string} text What to type
 * @param {string} label The option's label
 */
async function pick( page, input, text, label ) {
	await input.fill( text );
	await page.getByRole( 'option', { name: label } ).first().click();
}

test( 'the group editor creates, sets, qualifies, sources and removes values', async ( { page } ) => {
	await page.goto( '/wiki/Special:UserLogin?returnto=Item:Q1' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q1$/ );

	// Create: Add statement, instance of = number.
	await page.getByRole( 'button', { name: 'Add statement' } ).click();
	await pick( page, page.getByRole( 'combobox', { name: 'Add statement', exact: true } ), 'instance', 'instance of' );
	const editor = page.locator( '.ts-group-editor' );
	await expect( editor.getByRole( 'heading', { name: 'instance of' } ) ).toBeVisible();
	await pick( page, editor.getByRole( 'combobox', { name: 'instance of 1', exact: true } ), 'num', 'number' );
	const axe = await new AxeBuilder( { page } ).include( '.ts-group-editor' ).analyze();
	expect( axe.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.html ).join( ' | ' ) }` ) ).toEqual( [] );
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( editor ).toHaveCount( 0 );
	const group = page.locator( '[data-region="statements/P2"]' );
	await expect( group ).toContainText( 'number' );
	await matchesServer( page, 'P2' );

	// Set, qualify and source: number → city, with a point in time and a reference URL.
	await group.getByRole( 'button', { name: 'Edit the values of instance of' } ).click();
	await pick( page, editor.getByRole( 'combobox', { name: 'instance of 1', exact: true } ), 'cit', 'city' );
	await editor.getByRole( 'button', { name: 'Add qualifier' } ).click();
	await pick( page, editor.getByRole( 'combobox', { name: 'Add qualifier', exact: true } ), 'point', 'point in time' );
	await editor.getByLabel( 'point in time', { exact: true } ).fill( '2020' );
	await editor.getByRole( 'button', { name: 'Add source' } ).click();
	await pick( page, editor.getByRole( 'combobox', { name: 'Add source', exact: true } ), 'reference', 'reference URL' );
	await editor.getByLabel( 'reference URL', { exact: true } ).fill( 'https://example.org/q1' );
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( editor ).toHaveCount( 0 );
	await expect( group ).toContainText( 'city' );
	await expect( group ).toContainText( '2020' );
	await expect( group ).toContainText( 'https://example.org/q1' );
	await matchesServer( page, 'P2' );

	// Add a second value, then remove the first.
	await group.getByRole( 'button', { name: 'Edit the values of instance of' } ).click();
	await editor.getByRole( 'button', { name: 'Add value' } ).click();
	await pick( page, editor.getByRole( 'combobox', { name: 'instance of 2', exact: true } ), 'num', 'number' );
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( group ).toContainText( '2 values' );
	await group.getByRole( 'button', { name: 'Edit the values of instance of' } ).click();
	await editor.getByRole( 'button', { name: 'Remove value' } ).first().click();
	await expect( editor.getByText( 'This value will be removed when you save.' ) ).toBeVisible();
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( editor ).toHaveCount( 0 );
	await expect( group ).not.toContainText( 'city' );
	await expect( group ).toContainText( 'number' );
	await matchesServer( page, 'P2' );

	// A quantity, and an invalid one first.
	await page.getByRole( 'button', { name: 'Add statement' } ).click();
	await pick( page, page.getByRole( 'combobox', { name: 'Add statement', exact: true } ), 'popul', 'population' );
	await editor.getByLabel( 'population 1: Amount' ).fill( 'many' );
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( editor.getByText( 'population 1: enter a number' ) ).toBeVisible();
	await editor.getByLabel( 'population 1: Amount' ).fill( '1200' );
	await editor.getByRole( 'button', { name: 'Save' } ).click();
	await expect( page.locator( '[data-region="statements/P3"]' ) ).toContainText( '1,200' );
	await matchesServer( page, 'P3' );

	// All of it was saved.
	await page.reload();
	await expect( page.locator( '[data-region="statements/P2"]' ) ).toContainText( 'number' );
	await expect( page.locator( '[data-region="statements/P3"]' ) ).toContainText( '1,200' );
} );
