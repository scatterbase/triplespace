// Every page kind of Phase 1 with JavaScript off (ADR 0034 §1.1), checked by axe (§10),
// through an edge that alternates two web replicas, so a page that needs one replica's
// memory fails here.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * How far the page scrolls sideways: 0 unless something overflows.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<number>} The overflow in pixels
 */
function overflow( page ) {
	return page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
}

const pages = [
	{ path: '/wiki/Project:Home', heading: 'librarybase' },
	{ path: '/wiki/Special:SpecialPages', heading: 'Special pages' },
	{ path: '/wiki/Special:UserLogin', heading: 'Log in' },
	{ path: '/wiki/Project:About', heading: 'Project:About', status: 404 },
	{ path: '/wiki/Item:Q6', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=identifiers', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=sitelinks', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=labels', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?uselang=ar', heading: 'ستة' },
	{ path: '/wiki/Property:P3', heading: 'population' },
	{ path: '/wiki/Domain:wikipedia.org', heading: 'Wikipedia' },
	{ path: '/wiki/Item:Q404', heading: 'Item:Q404', status: 404 },
	{ path: '/w/index.php?title=Special:Search&search=six&fulltext=1', heading: 'Search' },
	{ path: '/wiki/Special:Search', heading: 'Search' }
];

for ( const p of pages ) {
	test( `${ p.path } reads without JavaScript`, async ( { page } ) => {
		const response = await page.goto( p.path );
		expect( response.status() ).toBe( p.status || 200 );
		await expect( page.locator( 'html' ) ).toHaveClass( /client-nojs/ );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( p.heading );
		expect( await overflow( page ) ).toBe( 0 );
	} );

	// axe runs as a script in the page, so this context allows scripts; the site's own
	// script only marks the page as having them.
	test( `${ p.path } passes axe`, async ( { browser } ) => {
		const context = await browser.newContext( { javaScriptEnabled: true } );
		const page = await context.newPage();
		await page.goto( p.path );
		const results = await new AxeBuilder( { page } )
			.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
			.analyze();
		const found = results.violations.map(
			( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
		);
		expect( found ).toEqual( [] );
		await context.close();
	} );
}

test( 'folds open and footnotes link without JavaScript', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	const fold = page.locator( '#P2 details.ts-fold' ).first();
	await expect( fold.getByText( 'city' ) ).toBeHidden();
	await fold.locator( 'summary' ).click();
	await expect( fold.getByText( 'city' ) ).toBeVisible();
	await page.locator( '#P3 a.ts-fn' ).click();
	await expect( page ).toHaveURL( /#P3-fn-1$/ );
	await expect( page.locator( '#P3-fn-1' ).getByText( 'reference URL' ) ).toBeVisible();
} );

test( 'the header box goes to an ID and searches words without JavaScript', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	const box = page.locator( 'header input[name="search"]' );
	await box.fill( 'P3' );
	await box.press( 'Enter' );
	await expect( page ).toHaveURL( /\/wiki\/Property:P3$/ );
	await box.fill( 'six' );
	await box.press( 'Enter' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Search' );
	await expect( page.locator( '.ts-hit__title' ).first() ).toHaveText( 'Six & more' );
} );

test( 'the main page is Project:Home, and unbuilt features are not linked', async ( { page } ) => {
	await page.goto( '/' );
	await expect( page ).toHaveURL( /\/wiki\/Project:Home$/ );
	await page.goto( '/wiki/Main_Page' );
	await expect( page ).toHaveURL( /\/wiki\/Project:Home$/ );
	for ( const absent of [ 'Special:RecentChanges', 'Special:Jobs', 'Special:NewItem', 'action=history' ] ) {
		await expect( page.locator( `a[href*="${ absent }"]` ) ).toHaveCount( 0 );
	}
	await page.goto( '/wiki/Item:Q6' );
	await expect( page.locator( 'a[href*="action=history"]' ) ).toHaveCount( 0 );
} );

test( 'logging in and out works without JavaScript, through either replica', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	await page.getByRole( 'link', { name: 'Log in' } ).click();
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Log in' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( 'not her password' );
	await page.getByRole( 'button', { name: 'Log in' } ).click();
	await expect( page.getByText( 'Incorrect username or password.' ) ).toBeVisible();
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await expect( page.locator( 'summary[aria-label="Account menu for Alice"]' ) ).toBeVisible();
	await page.locator( 'summary[aria-label="Account menu for Alice"]' ).click();
	await page.getByRole( 'link', { name: 'Log out' } ).click();
	await expect( page.getByText( 'Do you want to log out?' ) ).toBeVisible();
	await page.getByRole( 'button', { name: 'Log out' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await expect( page.getByRole( 'link', { name: 'Log in' } ) ).toBeVisible();
} );

test( 'a phone reads an item without scrolling sideways', async ( { browser } ) => {
	const context = await browser.newContext( {
		viewport: { width: 390, height: 844 },
		javaScriptEnabled: false
	} );
	const page = await context.newPage();
	await page.goto( '/wiki/Item:Q6' );
	expect( await overflow( page ) ).toBe( 0 );
	await context.close();
} );

test( 'consecutive requests alternate replicas and get the same page', async ( { request } ) => {
	const a = await request.get( '/wiki/Item:Q6' );
	const b = await request.get( '/wiki/Item:Q6' );
	expect( a.headers()[ 'x-replica' ] ).not.toBe( b.headers()[ 'x-replica' ] );
	expect( await a.text() ).toBe( await b.text() );
	expect( a.headers().etag ).toBe( b.headers().etag );
	const again = await request.get( '/wiki/Item:Q6', { headers: { 'if-none-match': a.headers().etag } } );
	expect( again.status() ).toBe( 304 );
} );

test( 'an anonymous page is public; a signed-in page is private', async ( { playwright, baseURL } ) => {
	const anon = await playwright.request.newContext( { baseURL } );
	const page = await anon.get( '/wiki/Item:Q6' );
	expect( page.headers()[ 'cache-control' ] ).toMatch( /^public/ );
	expect( page.headers()[ 'cache-tag' ] ).toContain( 'entity:Q6' );
	await anon.dispose();

	const alice = await playwright.request.newContext( { baseURL } );
	const tokens = await ( await alice.get(
		'/w/api.php?action=query&meta=tokens&type=login&format=json&formatversion=2'
	) ).json();
	const login = await ( await alice.post( '/w/api.php', {
		form: {
			action: 'clientlogin',
			format: 'json',
			formatversion: '2',
			username: 'Alice',
			password: process.env.E2E_OWNER_PASSWORD,
			loginreturnurl: `${ baseURL }/wiki/Project:Home`,
			logintoken: tokens.query.tokens.logintoken
		}
	} ) ).json();
	expect( login.clientlogin.status ).toBe( 'PASS' );
	for ( let i = 0; i < 2; i++ ) {
		// Both replicas know her: the session lives in the API, not in either replica.
		const mine = await alice.get( '/wiki/Item:Q6' );
		expect( mine.headers()[ 'cache-control' ] ).toBe( 'private, no-cache' );
		expect( mine.headers()[ 'cache-tag' ] ).toBeUndefined();
		expect( await mine.text() ).toContain( 'Account menu for Alice' );
	}
	await alice.dispose();
} );
