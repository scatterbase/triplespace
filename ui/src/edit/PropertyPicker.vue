<template>
	<div class="ts-property-picker">
		<CdxButton
			v-if="!open"
			type="button"
			:disabled="disabled"
			@click="start">
			{{ action }}
		</CdxButton>
		<div v-else class="ts-property-picker__open">
			<EntityLookup
				ref="lookup"
				:api="api"
				:lang="data.lang"
				type="property"
				:label="action"
				:placeholder="t( 'ts-edit-search-property' )"
				@pick="pick"
			/>
			<CdxButton
				type="button"
				weight="quiet"
				@click="open = false">
				{{ t( 'ts-edit-cancel' ) }}
			</CdxButton>
		</div>
	</div>
</template>

<script setup lang="ts">
// "Add qualifier", "Add source" and "Add statement": a button that opens a property
// search, and hands on the chosen property with its data type and label.
import { nextTick, ref } from 'vue';
import { CdxButton } from '@wikimedia/codex';
import type { Api } from './api';
import { msg } from './data';
import type { EditData } from './data';
import { describe } from './describe';
import EntityLookup from './EntityLookup.vue';

const props = defineProps<{
	data: EditData;
	api: Api;
	action: string;
	disabled?: boolean;
}>();

const emit = defineEmits<{
	chosen: [ property: string, datatype: string, label: string ];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );
const open = ref( false );
const lookup = ref<{ $el: HTMLElement } | null>( null );

async function start(): Promise<void> {
	open.value = true;
	await nextTick();
	lookup.value?.$el.querySelector( 'input' )?.focus();
}

async function pick( id: string, label: string ): Promise<void> {
	if ( !id ) {
		return;
	}
	const facts = await describe( props.api, props.data.lang, [ id ] );
	open.value = false;
	emit( 'chosen', id, facts.datatypes[ id ] ?? 'string', facts.labels[ id ] ?? label );
}
</script>
